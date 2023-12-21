use std::io::BufRead;
use std::str::FromStr;

use chrono::{NaiveDate, NaiveDateTime};
use colored::Colorize;
use csv::Reader;
use inquire::error::InquireResult;
use inquire::list_option::ListOption;
use inquire::{DateSelect, MultiSelect, Select};
use prettytable::{row, Table};
use serde::de::Error;
use serde::Deserialize;
use sqlx::{Pool, Sqlite, SqlitePool};
use titlecase::titlecase;

pub(crate) mod earned_value;

pub(crate) async fn list(
    pool: &SqlitePool,
    task_status: TaskStatus,
    number_of_tasks: &Option<usize>,
) -> anyhow::Result<()> {
    let tasks = get_tasks(pool.clone(), task_status).await?;

    let tasks = match number_of_tasks {
        Some(n) => tasks.into_iter().take(*n).collect(),
        None => tasks,
    };

    // give each assignee a color based on assignees in tasks
    let assignees = tasks
        .iter()
        .map(|task| task.assignee.clone().unwrap_or("".to_string()))
        .collect::<Vec<String>>();

    // Create the table
    let mut table = Table::new();
    table.add_row(row![
        "#".bold(),
        "Assignee".bold(),
        "Task".bold(),
        "Estimated Duration".bold(),
        "Slack".bold(),
        "Planned Start Date".bold(),
        "Planned Finish Date".bold(),
        "Actual Finish Date".bold(),
        "Predecessors".bold(),
    ]);
    for task in tasks {
        let assignee = titlecase(&*task.assignee.unwrap_or("".to_string()));

        let predecessor_string = task
            .predecessors
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<String>>()
            .join(",");

        let finished_at_string = match task.finished_at {
            Some(date) => dfmt(date),
            None => "--".to_string(),
        };

        let start_date = dfmt(task.start_date);
        let finish_date = dfmt(task.finish_date);

        let slack = if (task.slack <= 10) {
            task.slack.to_string().red()
        } else if task.slack <= 30 {
            task.slack.to_string().yellow()
        } else {
            task.slack.to_string().green()
        };

        if task.finished {
            table.add_row(row![
                task.id.to_string().dimmed(),
                assignee,
                task.name.green().dimmed(),
                task.duration.to_string().dimmed(),
                slack.dimmed(),
                start_date.dimmed(),
                finish_date.dimmed(),
                finished_at_string.dimmed(),
                predecessor_string.dimmed()
            ]);
        } else if assignee != "" {
            table.add_row(row![
                task.id.to_string().bold(),
                assignee,
                task.name.blue(),
                task.duration.to_string(),
                slack,
                start_date,
                finish_date.to_string(),
                finished_at_string,
                predecessor_string
            ]);
        } else {
            table.add_row(row![
                task.id.to_string(),
                assignee,
                task.name,
                task.duration.to_string(),
                slack,
                start_date.to_string(),
                finish_date.to_string(),
                finished_at_string,
                predecessor_string
            ]);
        };
    }
    table.printstd();
    Ok(())
}

async fn get_tasks(pool: Pool<Sqlite>, task_status: TaskStatus) -> anyhow::Result<Vec<Task>> {
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
    .fetch_all(&pool)
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
        TaskStatus::Assigned => tasks
            .into_iter()
            .filter(|task| task.assignee != None && task.finished == 0)
            .collect(),
        TaskStatus::Unassigned => tasks
            .into_iter()
            .filter(|task| task.assignee == None)
            .collect(),
    };

    // convert to task type
    let mut all_tasks: Vec<Task> = vec![];

    for t in tasks {
        let finished_at_date = t
            .finished_at
            .map(|date_str| NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S").ok())
            .flatten()
            .map(|naive_datetime| naive_datetime.date());

        all_tasks.push(Task {
            id: t.id,
            name: t.name,
            duration: t.duration,
            slack: t.total_slack,
            predecessors: t
                .predecessors
                .unwrap()
                .split(',')
                .map(|s| s.parse::<i64>().unwrap())
                .collect(),
            start_date: t.start_date.parse()?,
            finish_date: t.finish_date.parse()?,
            resource_names: t
                .resource_names
                .unwrap()
                .split(',')
                .map(|s| s.to_string())
                .collect(),
            pdex_criticality: t.pdex_criticality.unwrap_or(0),
            assignee: t.assignee,
            finished_at: finished_at_date,
            finished: t.finished == 1,
        })
    }

    Ok(all_tasks)
}
pub async fn log_work(pool: &SqlitePool) -> anyhow::Result<()> {
    // find tasks in progress
    let selected_task = select_task(
        get_tasks(pool.clone(), TaskStatus::Assigned).await?,
        "Select task to log work:",
    )
    .expect(
        "Error when selecting task to log work. Do you have assigned tasks that are in progress?",
    );

    let date = DateSelect::new("Select date: ")
        .prompt()
        .expect("Error in date selection")
        .format("%Y-%m-%d")
        .to_string();

    let days = Select::new("Log time worked (in days): ", vec![5, 4, 3, 2, 1])
        .prompt()
        .expect("Error in duration selection");

    // Insert the task, then obtain the ID of this row
    let id = sqlx::query!(
        r#"
            INSERT INTO timesheet (task_id, date, duration) VALUES (?1, ?2, ?3)
            "#,
        selected_task.id,
        date,
        days
    )
    .execute(pool)
    .await?
    .last_insert_rowid();

    println!(
        "⌛ Logged time on #{} - {} assigned to {} (rowid: {})",
        selected_task.id,
        selected_task.name,
        selected_task.assignee.unwrap_or("unknown".to_string()),
        id.to_string()
    );
    Ok(())
}

pub fn track(pool: &SqlitePool) {}
pub async fn import(pool: &SqlitePool, ms_project_file: String, database_file: String) {
    println!("Init {} {}", ms_project_file, database_file);
    let tasks = load_from_csv(&ms_project_file).expect(&*format!(
        "Failed to load tasks from CSV file {}",
        ms_project_file
    ));
    for task in &tasks {
        let _inserted_id = insert_task(&pool, &task)
            .await
            .expect("Failed to insert task");
    }
    println!("✨Imported {} tasks", tasks.len());
}

#[derive(Debug, Deserialize)]
struct MsProjectTask {
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

async fn insert_task(pool: &SqlitePool, task: &MsProjectTask) -> anyhow::Result<i64> {
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
fn load_from_csv(path: &str) -> Result<Vec<MsProjectTask>, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_path(path)?;
    let mut tasks: Vec<MsProjectTask> = Vec::new();

    for record in reader.deserialize() {
        let record: MsProjectTask = record?;
        tasks.push(record);
    }

    Ok(tasks)
}

#[derive(Debug)]
pub enum TaskStatus {
    Pending,
    Completed,
    All,
    Assigned,
    Unassigned,
}

pub(crate) async fn complete_tasks(pool: &Pool<Sqlite>) -> anyhow::Result<()> {
    // Insert the task, then obtain the ID of this row
    let tasks = sqlx::query!(
        r#"
        SELECT t.id, t.name, td.assignee as assignee FROM tasks t, task_data td WHERE t.id = td.task_id AND td.finished_at IS NULL
        "#
    )
    .fetch_all(pool)
    .await?;

    let options = tasks
        .iter()
        .map(|task| {
            ListOption::new(
                task.id as usize,
                format!(
                    "{} - ({})",
                    task.name.as_str(),
                    task.assignee.as_ref().unwrap_or(&"Unassigned".to_string())
                ),
            )
        })
        .collect();

    let tasks_to_complete = MultiSelect::new("Select tasks to complete:", options).prompt();

    match tasks_to_complete {
        Ok(..) => {
            let tasks = tasks_to_complete.expect("Error in selection");
            for task in tasks {
                let task_id = task.index as i32;
                sqlx::query!(
                    r#"
                    UPDATE task_data SET finished_at = CURRENT_TIMESTAMP WHERE task_id = ?1
                    "#,
                    task_id
                )
                .execute(pool)
                .await?;
                println!("✨Completed task #{} - {}", task_id, task.value);
            }
        }
        Err(_) => println!("Error in selection"),
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Task {
    id: i64,
    name: String,
    duration: i64,
    slack: i64,
    predecessors: Vec<i64>,
    start_date: NaiveDate,
    finish_date: NaiveDate,
    resource_names: Vec<String>,
    pdex_criticality: i64,
    assignee: Option<String>,
    finished_at: Option<NaiveDate>,
    finished: bool,
}

pub(crate) async fn assign_tasks(pool: Pool<Sqlite>) -> anyhow::Result<()> {
    let tasks_to_assign = select_tasks(
        get_tasks(pool.clone(), TaskStatus::Unassigned).await?,
        "Select tasks to assign:",
        20,
    )
    .expect("Error when selecting tasks to assign");

    // Get assignees from env variable comma separated
    let team_members = std::env::var("PROJECT_MANAGER_TEAM_MEMBERS")
        .expect("PROJECT_MANAGER_TEAM_MEMBERS not set")
        .split(",")
        .map(|s| titlecase(s.trim()))
        .collect::<Vec<String>>();

    let ans: InquireResult<String> = Select::new("Select assignee: ", team_members).prompt();
    let assignee = match ans {
        Ok(assignee) => Some(assignee),
        Err(_) => None,
    }
    .expect("No assignee selected");

    for task in tasks_to_assign {
        // Insert the task, then obtain the ID of this row
        let id = sqlx::query!(
            r#"
        INSERT INTO task_data (assignee, task_id) VALUES (LOWER(?1), ?2)
        "#,
            assignee,
            task.id
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        println!(
            "✨Assigned task #{} - {} to {} (rowid: {})",
            task.id,
            task.name,
            assignee,
            id.to_string()
        );
    }

    Ok(())
}

fn select_task(tasks: Vec<Task>, prompt: &str) -> Option<Task> {
    let options: Vec<ListOption<String>> = tasks
        .iter()
        .map(|task| {
            ListOption::new(
                task.id as usize,
                format!(
                    "#{} - {} (start: {})",
                    task.id,
                    task.name.as_str(),
                    task.start_date.format("%d.%m.%y")
                ),
            )
        })
        .collect();

    let ans: InquireResult<ListOption<String>> = Select::new(prompt, options).prompt();

    match ans {
        Ok(task) => Some(
            tasks
                .iter()
                .find(|t| t.id == task.index as i64)
                .unwrap()
                .clone(),
        ),
        Err(_) => None,
    }
}

fn select_tasks(tasks: Vec<Task>, prompt: &str, page_size: usize) -> Option<Vec<Task>> {
    let options: Vec<ListOption<String>> = tasks
        .iter()
        .map(|task| {
            ListOption::new(
                task.id as usize,
                format!(
                    "#{} - {} (start: {})",
                    task.id,
                    task.name.as_str(),
                    task.start_date.format("%d.%m.%y")
                ),
            )
        })
        .collect();

    let selected: InquireResult<Vec<ListOption<String>>> = MultiSelect::new(prompt, options)
        .with_page_size(page_size)
        .prompt();

    match selected {
        Ok(selected_tasks) => Some(
            selected_tasks
                .iter()
                .map(|selected_task| {
                    tasks
                        .iter()
                        .find(|t| selected_task.index == t.id as usize)
                        .unwrap()
                        .clone()
                })
                .collect(),
        ),
        Err(_) => None,
    }
}

fn dfmt(date: NaiveDate) -> String {
    date.format("%a %d.%m.%y").to_string()
}
