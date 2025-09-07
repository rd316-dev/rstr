use rusqlite::Transaction;

use crate::db::db::Table;

const SERVER_CONFIG_SCHEMA: Table = Table {
    name: "server_config",
    version: 1
};

const CLIENT_CONFIG_SCHEMA: Table = Table {
    name: "client_config",
    version: 1
};

const SERVER_LIST_SCHEMA: Table = Table {

}

const METADATA_SCHEMA: Table = Table {
    name: "metadata",
    version: 1
};

const CHUNK_SCHEMA: Table = Table {
    name: "chunk",
    version: 1
};