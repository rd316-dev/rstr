use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Transaction};

#[derive(Debug)]
pub enum DatabaseError<T = rusqlite::Error> {
    Initialization(rusqlite::Error),
    TransactionOpen(rusqlite::Error),
    TransactionCommit(rusqlite::Error),
    TransactionRollback(rusqlite::Error),
    MigrationError(rusqlite::Error),
    QueryError(T)
}

impl From<rusqlite::Error> for DatabaseError<rusqlite::Error> {
    fn from(error: rusqlite::Error) -> Self {
        DatabaseError::QueryError(error)
    }
}

pub struct Database {
    conn: Connection
}

unsafe impl Sync for Database {}

impl Database {
    pub fn init(path: &PathBuf) -> Result<Self, DatabaseError> {
        let conn = Connection::open(path)
            .map_err(|error| DatabaseError::Initialization(error))?;

        let mut database = Database {
            conn
        };

        /*database.tx(|tx| {
            tx.execute("
                CREATE TABLE schema_version (
                    name: TEXT PRIMARY KEY,
                    version: INTEGER
                )", ())?;

            Ok(())
        })?;*/

        Ok(database)
    }

    pub fn tx<T, E, U: Fn(&Transaction) -> Result<T, DatabaseError<E>>>(&mut self, block: U) -> Result<T, DatabaseError<E>> {
        let conn = &mut self.conn;
        let tx = conn.transaction().map_err(|error| DatabaseError::TransactionOpen(error))?;

        match block(&tx) {
            Ok(value) => {
                tx.commit().map_err(|error| DatabaseError::TransactionCommit(error))?;
                Ok(value)
            },
            Err(error) => {
                tx.rollback().map_err(|error| DatabaseError::TransactionRollback(error))?;
                Err(error)
            }
        }
    }
}

pub struct Table {
    pub name: &'static str,
    pub version: i32
}

impl Table {
    pub fn declare_schema(self, tx: &Transaction) -> Result<(), rusqlite::Error> {
        struct TableVersion {
            name: String,
            version: i32
        }

        let existing_row = tx.query_row(
            "SELECT (schema, name, version) FROM schema_version WHERE name = ?1", 
            [self.name], 
            |row| { Ok( TableVersion { name: row.get(0)?, version: row.get(1)? })
        }).optional()?;

        match existing_row {
            Some( TableVersion { name, version} ) if version != self.version => {
                panic!("the schema version {} of the table '{}' doesn't match the declared version {}",
                    version, name, self.version);
            },
            None => {
                tx.execute("INSERT INTO schema_version(name, version) VALUES (?1, ?2)", 
                    (self.name, self.version))?;
            },
            _ => {}
        }

        Ok(())
    }
}

trait Dao {
    fn migrate(tx: &Transaction) -> Result<(), DatabaseError>;
    fn init(tx: &Transaction) -> Result<(), DatabaseError>;
}

pub struct ServerDao;

impl ServerDao {

    pub fn init(tx: &Transaction) -> Result<(), DatabaseError> {
        tx.execute("
            CREATE TABLE IF NOT EXISTS server_connection (
                id: INTEGER PRIMARY KEY,
                url: TEXT,
                role: TEXT,
                username: TEXT,
                password: TEXT
            );
        ", ())?;

        Ok(())
    }
}

pub struct MetadataDao;

impl MetadataDao {

    pub fn init(tx: &Transaction) -> Result<(), DatabaseError> {
        tx.execute("
            CREATE TABLE IF NOT EXISTS metadata (
                id: INTEGER PRIMARY KEY,
                full_path: TEXT CONSTRAINT UNIQUE,
                hash: INTEGER,
                size: INTEGER,
                file_modified: INTEGER,
                meta_modified: INTEGER        
            );

            CRETE TABLE chunk (
                id: INTEGER PRIMARY KEY,
                metadata: INTEGER,
                hash: INTEGER,
                offset: INTEGER,
                size: INTEGER,

                FOREIGN KEY(metadata) REFERENCES metadata(id)
            );
        ", ())?;

        Ok(())
    }
}