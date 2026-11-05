# PostgreSQL development setup

The backend now uses PostgreSQL. SQLite is no longer used by the application.

## 1. Start PostgreSQL

From the repository root:

```bash
docker compose up -d postgres
```

Check that the database is ready:

```bash
docker compose ps
docker compose logs postgres
```

The development database is:
- Database: `rust_quant`
- User: `postgres`
- Password: `postgres`
- Host port: `5432`

These credentials are for local development only. Change them before exposing the service.

## 2. Configure the backend

From the repository root, export the connection string:

```bash
export DATABASE_URL='postgres://postgres:postgres@localhost:5432/rust_quant'
```

Or copy `apps/backend/.env.example` values into your shell environment. The application currently reads environment variables directly and does not load a `.env` file automatically.

## 3. Run the backend

```bash
cargo run -p quant-backend
```

At startup, SQLx applies pending migrations from `apps/backend/migrations`. The migration creates the task table and an index for queued-task polling.

## 4. Existing SQLite data

This change switches the backend to a new PostgreSQL database. It does not automatically copy existing rows from the old SQLite file. Existing SQLite task history remains in the old local database file and can be migrated separately if needed.
