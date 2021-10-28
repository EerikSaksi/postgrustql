#[cfg(test)]
#[path = "./test.rs"]
mod test;
use postgres::{Client, NoTls, Row};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

pub use self::generate_sql::ServerSidePoggers;
pub mod generate_sql;

#[derive(PartialEq, Debug, Clone)]
pub struct TableQueryInfo {
    graphql_fields: Vec<String>,
    parent_key_name: String,
    column_offset: usize,
}

pub fn run_multithreaded(gql_query: &str, pogg: &mut ServerSidePoggers) {
    let mut handles = vec![];

    let times: Vec<u128> = vec![];
    let (query, table_query_infos) = pogg.build_root(gql_query).unwrap();
    let mut thread_infos = (0..8).map(|_| (query.to_string(), table_query_infos.to_vec()));

    let client = Arc::new(Mutex::new((Client::connect(
        "postgres://eerik:Postgrizzly@localhost:5432/pets",
        NoTls,
    )
    .unwrap(),)));

    let runtime_infos = Arc::new(Mutex::new((times, Instant::now())));
    for _ in 0..8 {
        let client = Arc::clone(&client);
        let runtime_infos = Arc::clone(&runtime_infos);
        let threads_metadata = thread_infos.next().unwrap();
        let query = threads_metadata.0;
        let table_query_infos = threads_metadata.1;
        let handle = thread::spawn(move || loop {
            let rows: Vec<Row>;
            {
                let mut locked_client = client.lock().unwrap();
                rows = locked_client.0.query(&*query, &[]).unwrap();
            }
            convert(rows, &table_query_infos);
            let mut locked_runtime_infos = runtime_infos.lock().unwrap();
            if 1000 <= locked_runtime_infos.0.len() {
                return;
            }
            let elapsed = locked_runtime_infos.1.elapsed().as_micros();
            locked_runtime_infos.0.push(elapsed);
            locked_runtime_infos.1 = Instant::now();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
    println!("Multithreaded times {:?}", runtime_infos.lock().unwrap().0);
}

pub fn convert(rows: Vec<Row>, table_query_infos: &Vec<TableQueryInfo>) -> String {
    let mut s = [
        "{",
        &stringify(&table_query_infos.get(0).unwrap().parent_key_name),
        ":[\n",
    ]
    .concat();

    let table_index = 0;

    let mut row_iter = rows.iter();
    let first_row = row_iter.next().unwrap();
    s.push('{');
    build_parent(&mut s, &table_query_infos, table_index, &first_row);
    build_child(&mut s, &table_query_infos, table_index, &first_row);

    let mut last_pk: i32 = first_row.get(table_query_infos.get(1).unwrap().column_offset - 1);
    while let Some(row) = row_iter.next() {
        //one left of the start of the next tables cols is primary key
        let pk_index = table_query_infos
            .get(table_index + 1)
            .unwrap()
            .column_offset
            - 1;
        let pk: i32 = row.get(pk_index);
        if pk != last_pk {
            //parent changed
            s.drain(s.len() - 2..s.len());
            s.push_str(&["\n]\n},\n{\n"].concat());
            build_parent(&mut s, &table_query_infos, table_index, &row)
        }
        last_pk = pk;
        build_child(&mut s, &table_query_infos, table_index, row);
    }

    s.drain(s.len() - 2..s.len());

    s.push_str("]}]}");
    s
}
fn build_parent(
    s: &mut String,
    table_query_infos: &Vec<TableQueryInfo>,
    table_index: usize,
    row: &Row,
) {
    let col_offset = table_query_infos.get(0).unwrap().column_offset;
    for (i, gql_field) in table_query_infos
        .get(0)
        .unwrap()
        .graphql_fields
        .iter()
        .enumerate()
    {
        let col_val: i32 = row.get(col_offset + i);
        s.push_str(&[&stringify(gql_field), ":", &col_val.to_string(), ",\n"].concat());
    }

    s.push_str(
        &[
            &stringify(
                &table_query_infos
                    .get(table_index + 1)
                    .unwrap()
                    .parent_key_name,
            ),
            ":[\n",
        ]
        .concat(),
    )
}
fn build_child(
    s: &mut String,
    table_query_infos: &Vec<TableQueryInfo>,
    table_index: usize,
    row: &Row,
) {
    let col_offset = table_query_infos
        .get(table_index + 1)
        .unwrap()
        .column_offset;
    s.push_str("{\n");
    for (i, gql_field) in table_query_infos
        .get(1)
        .unwrap()
        .graphql_fields
        .iter()
        .enumerate()
    {
        let col_val: i32 = row.get(col_offset + i);
        s.push_str(&[&stringify(&gql_field), ":", &col_val.to_string(), ",\n"].concat());
    }
    s.drain(s.len() - 2..s.len());
    s.push_str("},\n");
}
fn stringify(field: &str) -> String {
    ["\"", field, "\""].concat()
}
