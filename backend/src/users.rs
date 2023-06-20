use std::fmt;

use crate::FlukeDb;
use rocket::http::Status;
use rocket::response::status::Created;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{fairing, http, Request, Response};
use rocket::{fairing::AdHoc, routes};
use rocket_db_pools::{sqlx, Connection};
use sqlx::FromRow;

type Result<T, E = rocket::response::Debug<sqlx::Error>> = std::result::Result<T, E>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateUserSchema {
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub password: String,
}

// Likely want to add 'Optional' fields for last name
// If Optional fields added, change .fetch_* to .fetch_optional(...)
#[derive(Debug, Clone, Deserialize, Serialize, FromRow, FromForm)]
pub struct UserModel {
    pub id: i64,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug)]
pub enum SignupError {
    NonUniqueIdError,
    UnknownQueryError,
    UnknownDatabaseError,
}

impl From<sqlx::Error> for SignupError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::Database(db_error) => {
                let pg_error = db_error.downcast::<sqlx::postgres::PgDatabaseError>();
                match pg_error.code() {
                    "23505" => {
                        println!("Duplicate user ID.");
                        SignupError::NonUniqueIdError
                    }
                    _ => {
                        println!("-- An error the server didn't account for --");
                        println!("{:?}", pg_error);
                        SignupError::UnknownQueryError
                    }
                }
            }
            _ => {
                println!("Something else happened");
                SignupError::UnknownDatabaseError
            }
        }
    }
}

impl std::fmt::Display for SignupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignupError::NonUniqueIdError => {
                write!(f, "Duplicate username or email contained a duplicate key.")
            }
            SignupError::UnknownQueryError => {
                write!(f, "Database query contained an unspecified error.")
            }
            SignupError::UnknownDatabaseError => write!(f, "Database error, not query related."),
        }
    }
}

pub struct CORS;
#[rocket::async_trait]
impl fairing::Fairing for CORS {
    fn info(&self) -> fairing::Info {
        fairing::Info {
            name: "Add CORS headers to responses",
            kind: fairing::Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(http::Header::new(
            "Access-Control-Allow-Origin",
            request.headers().get_one("Origin").unwrap_or("*"),
        ));
        response.set_header(http::Header::new(
            "Access-Control-Allow-Methods",
            "POST, GET, PATCH, OPTIONS",
        ));
        response.set_header(http::Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(http::Header::new(
            "Access-Control-Allow-Credentials",
            "true",
        ));
    }
}

#[get("/users")]
pub async fn list_users(mut db: Connection<FlukeDb>) -> Result<Json<Vec<UserModel>>> {
    let list_of_users: Vec<UserModel> = sqlx::query_as!(UserModel, "SELECT * FROM user_profile")
        .fetch_all(&mut *db)
        .await?;

    Ok(Json(list_of_users))
}

pub async fn create_user(
    mut db: Connection<FlukeDb>,
    user: CreateUserSchema,
) -> Result<UserModel, SignupError> {
    let user_model: UserModel = sqlx::query_as!(
        UserModel,
        r#"
        INSERT INTO user_profile (username, first_name, last_name, email, password)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
        user.username,
        user.first_name,
        user.last_name,
        user.email,
        user.password
    )
    .fetch_one(&mut *db)
    .await
    .map_err(SignupError::from)?;

    Ok(user_model)
}

#[post("/signup", data = "<user>")]
async fn signup_user(
    db: Connection<FlukeDb>,
    user: Json<CreateUserSchema>,
) -> Result<Created<Json<UserModel>>, rocket::response::status::Custom<String>> {
    match create_user(db, user.into_inner()).await {
        Ok(user_model) => {
            let location = format!("/users/{}", user_model.id);
            Ok(Created::new(location).body(Json(user_model)))
        }
        Err(e) => {
            let status = match e {
                SignupError::NonUniqueIdError => Status::Conflict,
                _ => Status::InternalServerError,
            };
            Err(rocket::response::status::Custom(status, e.to_string()))
        }
    }
}

#[delete("/users/<id>")]
pub async fn delete_user(mut db: Connection<FlukeDb>, id: i64) -> Result<Option<()>> {
    let result: sqlx::postgres::PgQueryResult =
        sqlx::query!("DELETE FROM user_profile WHERE id = $1", id)
            .execute(&mut *db)
            .await?;

    Ok((result.rows_affected() == 1).then(|| ()))
}

#[get("/<id>")]
pub async fn get_user(mut db: Connection<FlukeDb>, id: i64) -> Result<Option<Json<UserModel>>> {
    let user: Option<UserModel> =
        sqlx::query_as!(UserModel, "SELECT * FROM user_profile WHERE id = $1", id)
            .fetch_optional(&mut *db)
            .await?;

    Ok(user.map(Json))
}

pub fn users_stage() -> AdHoc {
    AdHoc::on_ignite("Users Stage", |rocket| async {
        rocket
            .mount("/users/", routes![list_users, delete_user, get_user])
            .mount("/", routes![signup_user])
            .attach(CORS)
    })
}
