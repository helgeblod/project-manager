extern crate clap;
extern crate prettytable;

use chrono::format::Numeric::IsoWeek;
use chrono::{Datelike, Local};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::{Parser, Subcommand};
use colored::Colorize;
use inquire::{InquireError, Select};
use promptly::prompt_default;
use sqlx::migrate::MigrateDatabase;
use sqlx::sqlite::SqlitePool;
use sqlx::{Pool, Sqlite};

use project::{earned_value, TaskStatus};

mod project;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Project management done right in the terminal 👔 \n Database file is read from the ENV-variable TASKS_DB_FILE or defaults to tasks.db",
    long_about = "Project management tool to support The Method from IDesign"
)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the database from from MS Project
    Init {},

    /// List tasks ✅
    List {},

    /// Log work 📝
    Log {},

    /// Generate earned value chart 📈
    EarnedValue {
        /// The path to the generated chart
        #[arg(short, long, default_value = "earned_value.png")]
        outfile: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let database_file =
        std::env::var("PROJECT_MANAGER_DB_FILE").unwrap_or("./db/tasks.db".to_string());

    match &cli.command {
        Commands::Init {} => {
            // Migrate database
            create_database_check(&database_file).await?;
            let pool = connect_to_db_pool(&database_file).await;
            sqlx::migrate!().run(&pool).await?;
            // Prompt Y/n with a default value when input is empty
            let confirm_continue = prompt_default("💩 Continue importing from MS Project?", false);
            if confirm_continue.is_err() || confirm_continue.unwrap() != true {
                println!("👋 Bye!");
                return Ok(());
            }

            let project_file: PathBuf =
                prompt_default("Enter path to MS Project file", PathBuf::from("tasks.csv"))?;

            // get command
            project::init(
                &pool,
                project_file.to_string_lossy().into_owned(),
                database_file,
            )
            .await;
        }

        Commands::List { .. } => {
            let pool = connect_to_db_pool(&database_file).await;

            let options: Vec<&str> = vec!["Doing/Pending", "All", "Completed"];
            let ans: Result<&str, InquireError> = Select::new("What tasks?", options).prompt();

            let choice = match ans {
                Ok(choice) => match choice {
                    "Doing/Pending" => TaskStatus::Pending,
                    "All" => TaskStatus::All,
                    "Completed" => TaskStatus::Completed,
                    _ => TaskStatus::All,
                },
                Err(_) => TaskStatus::All,
            };
            project::list(&pool, choice)
                .await
                .expect("Could not list tasks");
        }

        Commands::Log { .. } => {
            let pool = connect_to_db_pool(&database_file).await;
            project::log(&pool);
        }

        Commands::EarnedValue { .. } => {
            let pool = connect_to_db_pool(&database_file).await;
            let today = Local::now();
            let prefixed_file_name = format!(
                "charts/ev_chart-week-{}-({}).png",
                today.iso_week().week(),
                today.format("%s").to_string()
            );
            let path = PathBuf::from(prefixed_file_name);
            let earned_value_file: PathBuf =
                prompt_default("Enter path to generated chart:", path)?;

            fs::create_dir_all(&earned_value_file.parent().unwrap().to_path_buf())?;
            earned_value::generate_chart(&pool, earned_value_file)
                .await
                .expect("Could not generate chart 💥");
        }
    }
    Ok(())
}

async fn connect_to_db_pool(database_file: &String) -> Pool<Sqlite> {
    let pool = SqlitePool::connect(&*db_url(database_file))
        .await
        .expect(&*format!(
            "Could not connect to database {}, check if correct folder!",
            database_file
        ));
    pool
}

async fn create_database_check(database_file: &String) -> anyhow::Result<()> {
    if !Sqlite::database_exists(&*db_url(database_file)).await? {
        // Create the parent directory if it doesn't exist
        if let Some(parent_dir) = Path::new(&database_file).parent() {
            fs::create_dir_all(parent_dir)?;
        }
        Sqlite::create_database(&*db_url(database_file)).await?;
    } else {
        println!(
            "👋 Database exists, remove and re-run init (using db: {}).",
            database_file
        );
        exit(1);
    }
    Ok(())
}

fn db_url(database_file: &String) -> String {
    return format!("sqlite:{}", database_file);
}
