use chrono::NaiveDate;
use colored::Colorize;
use csv::Reader;
use prettytable::{row, Table};
use serde::de::Error;
use serde::Deserialize;
use sqlx::SqlitePool;
use titlecase::titlecase;

pub(crate) mod earned_value;

pub(crate) async fn list(pool: &SqlitePool, task_status: TaskStatus) -> anyhow::Result<()> {
    println!("list");
    let conn = pool.acquire().await?;

    // Insert the task, then obtain the ID of this row
    let tasks = sqlx::query!(
        r#"
    SELECT t.id           as id,
       t.name             as name,
       t.duration         as duration,
       t.predecessors     as predecessors,
       t.start_date       as start_date,
       t.finish_date      as finish_date,
       t.total_slack      as total_slack,
       t.resource_names   as resource_names,
       t.pdex_criticality as pdex_criticality,
       td.id              as task_data_id,
       td.assignee        as assignee,
       td.finished_at     as finished_at,
       CASE
           WHEN td.finished_at IS NOT NULL THEN true
           ELSE false
       END as finished
       FROM tasks t
         LEFT OUTER JOIN task_data td
                         ON t.id = td.task_id
       WHERE duration > 0
       ORDER BY start_date, total_slack DESC;
    "#,
    )
    .fetch_all(pool)
    .await?;

    // Filter tasks based on status
    let tasks = match task_status {
        TaskStatus::All => tasks,
        TaskStatus::Pending => tasks
            .into_iter()
            .filter(|task| task.finished == 0)
            .collect(),
        TaskStatus::Completed => tasks
            .into_iter()
            .filter(|task| task.finished == 1)
            .collect(),
    };

    // Create the table
    let mut table = Table::new();
    table.add_row(row!["ID".bold(), "Assignee".bold(), "Task".bold()]);
    for task in tasks {
        let assignee = titlecase(&*task.assignee.unwrap_or("".to_string()));
        if task.finished == 1 {
            table.add_row(row![
                task.id.to_string().bold(),
                assignee,
                task.name.green().dimmed()
            ]);
        } else if assignee != "" {
            table.add_row(row![
                task.id.to_string().bold(),
                assignee,
                task.name.blue().bold()
            ]);
        } else {
            table.add_row(row![
                task.id.to_string().bold(),
                assignee,
                task.name.white().dimmed()
            ]);
        };
    }
    table.printstd();
    Ok(())
}

pub fn log(pool: &SqlitePool) {
    println!("Log")
}

pub fn track(pool: &SqlitePool) {}
pub async fn init(pool: &SqlitePool, ms_project_file: String, database_file: String) {
    println!("Init {} {}", ms_project_file, database_file);
    let tasks = load_from_csv(&ms_project_file).expect(&*format!(
        "Failed to load tasks from CSV file {}",
        ms_project_file
    ));
    for task in tasks {
        println!("Task: {:?}", task);
        let inserted_id = insert_task(&pool, &task)
            .await
            .expect("Failed to insert task");
        println!("Task inserted: {:?} with db-id: {}", task, inserted_id);
    }
}

#[derive(Debug, Deserialize)]
struct Task {
    #[serde(rename = "ID")]
    id: i32,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Duration", deserialize_with = "parse_days")]
    duration: i32,
    #[serde(rename = "Predecessors")]
    predecessors: String,
    #[serde(rename = "Start_Date", deserialize_with = "parse_date")]
    start_date: NaiveDate,
    #[serde(rename = "Finish_Date", deserialize_with = "parse_date")]
    finish_date: NaiveDate,
    #[serde(rename = "Total_Slack", deserialize_with = "parse_days")]
    total_slack: i32,
    #[serde(rename = "Resource_Names")]
    resource_names: String,
    #[serde(rename = "PDEx_Criticality")]
    pdex_criticality: i32,
}

//Parsing the total_slack field from a string like "30 days" into an integer.
fn parse_days<'de, D>(deserializer: D) -> Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.split_whitespace()
        .next()
        .unwrap()
        .parse::<i32>()
        .map_err(D::Error::custom)
}

fn parse_date<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&s, "%a %m/%d/%y").map_err(D::Error::custom)
}

async fn insert_task(pool: &SqlitePool, task: &Task) -> anyhow::Result<i64> {
    let mut conn = pool.acquire().await?;
    let start_date = task.start_date.format("%Y-%m-%d").to_string();
    let finish_date = task.finish_date.format("%Y-%m-%d").to_string();

    // Insert the task, then obtain the ID of this row
    let id = sqlx::query!(
    r#"
    INSERT INTO tasks (id, name, duration, predecessors, start_date, finish_date, total_slack, resource_names, pdex_criticality)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
    "#,
    task.id, task.name, task.duration, task.predecessors, start_date, finish_date, task.total_slack, task.resource_names, task.pdex_criticality
)
        .execute(&mut *conn)
        .await?
        .last_insert_rowid();
    Ok(id)
}

// Function that retrieves Tasks from CSV file
fn load_from_csv(path: &str) -> Result<Vec<Task>, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_path(path)?;
    let mut tasks: Vec<Task> = Vec::new();

    for record in reader.deserialize() {
        let record: Task = record?;
        tasks.push(record);
    }

    Ok(tasks)
}

#[derive(Debug)]
pub enum TaskStatus {
    Pending,
    Completed,
    All,
}
