CREATE TABLE IF NOT EXISTS tasks
(
    id               INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    duration         INTEGER NOT NULL,
    predecessors     TEXT,
    start_date       TEXT    NOT NULL,
    finish_date      TEXT    NOT NULL,
    total_slack      INTEGER NOT NULL,
    resource_names   TEXT,
    pdex_criticality INTEGER
);

CREATE TABLE IF NOT EXISTS task_data
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     INTEGER,
    assignee    TEXT,
    finished_at TEXT DEFAULT NULL,
    UNIQUE (task_id),
    FOREIGN KEY (task_id) REFERENCES tasks (id)
);

CREATE UNIQUE INDEX idx_task_id ON task_data(task_id);

CREATE TABLE IF NOT EXISTS timesheet
(
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER,
    date    TEXT,
    duration   INTEGER,
    FOREIGN KEY (task_id) REFERENCES tasks (id)
);
