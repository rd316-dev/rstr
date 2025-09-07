extern crate num_derive;

pub mod message;
pub mod meta;

pub mod db {
    pub mod db;
    mod schema;
}

pub mod binary {
    pub mod reader;
    pub mod writer;
    pub mod serialization;
}