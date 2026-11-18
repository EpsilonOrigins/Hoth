//! SQLite catalog initialization and forward migrations.

use rusqlite::{Connection, TransactionBehavior};
use std::{error::Error, fmt, path::Path};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_catalog.sql"),
    include_str!("../migrations/0002_catalog_indexes.sql"),
    include_str!("../migrations/0003_space_track_checkpoints.sql"),
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

#[derive(Debug)]
pub enum SchemaError {
    Sqlite(rusqlite::Error),
    TransactionActive,
    ForeignKeysDisabled,
    UnsupportedVersion {
        found: i64,
    },
    Migration {
        version: i64,
        source: rusqlite::Error,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(source) => write!(formatter, "catalog SQLite operation failed: {source}"),
            Self::TransactionActive => {
                formatter.write_str("finish the active transaction before migrating the catalog")
            }
            Self::ForeignKeysDisabled => {
                formatter.write_str("catalog connection could not enable SQLite foreign keys")
            }
            Self::UnsupportedVersion { found } => write!(
                formatter,
                "catalog schema version {found} is unsupported (expected 0 through {SCHEMA_VERSION})"
            ),
            Self::Migration { version, source } => {
                write!(formatter, "catalog migration {version} failed: {source}")
            }
        }
    }
}

impl Error for SchemaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(source) | Self::Migration { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for SchemaError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sqlite(source)
    }
}

/// Opens or creates a writable catalog, enables foreign keys, and migrates it.
pub fn open(path: impl AsRef<Path>) -> Result<Connection, SchemaError> {
    let mut connection = Connection::open(path)?;
    migrate(&mut connection)?;
    Ok(connection)
}

/// Enables foreign keys on a writable connection and applies pending migrations.
/// All pending changes and the version update commit in one transaction.
pub fn migrate(connection: &mut Connection) -> Result<(), SchemaError> {
    if !connection.is_autocommit() {
        return Err(SchemaError::TransactionActive);
    }

    connection.pragma_update(None, "foreign_keys", true)?;
    let foreign_keys: bool =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if !foreign_keys {
        return Err(SchemaError::ForeignKeysDisabled);
    }

    // Read the version after acquiring the writer lock so two openers cannot
    // both decide to apply the same migration.
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if !(0..=SCHEMA_VERSION).contains(&current) {
        return Err(SchemaError::UnsupportedVersion { found: current });
    }

    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = (index + 1) as i64;
        transaction
            .execute_batch(sql)
            .and_then(|()| transaction.pragma_update(None, "user_version", version))
            .map_err(|source| SchemaError::Migration { version, source })?;
    }
    transaction.commit()?;
    Ok(())
}
