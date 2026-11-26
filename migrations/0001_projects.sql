CREATE TABLE projects (
    id INTEGER PRIMARY KEY NOT NULL,
    owner TEXT NOT NULL,
    title TEXT NOT NULL
);
CREATE INDEX projects_owner_id ON projects (owner, id);
