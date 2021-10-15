#[cfg(test)]
#[path = "./test.rs"]
mod test;
use crate::handle_query::postgres_query_builder::PostgresBuilder;
use crate::internal_schema_info::{GraphQLEdgeInfo, GraphQLType, QueryEdgeInfo};
use async_graphql_parser::types::{DocumentOperations, Selection, SelectionSet};
use async_graphql_parser::{parse_query, Positioned};
use async_graphql_value::Value;
use petgraph::graph::{DiGraph, WalkNeighbors};
use petgraph::prelude::NodeIndex;
use std::collections::HashMap;
pub mod postgres_query_builder;

pub struct Poggers<SQL: postgres_query_builder::GraphQLQueryBuilder> {
    pub g: DiGraph<GraphQLType, GraphQLEdgeInfo>,
    pub query_to_type: HashMap<String, QueryEdgeInfo>,
    pub local_id: u8,
    pub query_builder: SQL,
}

#[allow(dead_code)]
impl<SQL: postgres_query_builder::GraphQLQueryBuilder> Poggers<SQL> {
    pub fn new(
        g: DiGraph<GraphQLType, GraphQLEdgeInfo>,
        query_to_type: HashMap<String, QueryEdgeInfo>,
    ) -> Poggers<postgres_query_builder::PostgresBuilder> {
        Poggers {
            g,
            query_to_type,
            local_id: 0,
            query_builder: PostgresBuilder {},
        }
    }

    pub fn build_root(&mut self, query: &str) -> Result<String, async_graphql_parser::Error> {
        let ast = parse_query::<&str>(query)?;
        match ast.operations {
            DocumentOperations::Single(Positioned { node, pos: _ }) => {
                Ok(self.visit_query(node.selection_set))
            }
            DocumentOperations::Multiple(_) => {
                panic!("DocumentOperations::Multiple(operation)")
            }
        }
    }
    fn visit_query(&mut self, selection_set: Positioned<SelectionSet>) -> String {
        let mut query_string = SQL::sql_query_header();

        //create a __local__ string that we can use to distinguish this selection, and increment
        //the local_id to ensure that this stays as unique
        let table_alias = SQL::table_alias(self.local_id);

        if let Selection::Field(field) = &selection_set.node.items.get(0).unwrap().node {
            let node_index;
            let is_many;

            //we need to wrap this so that query_type is dropped, and copy out the is_many and
            //node_index fields to satisfy the borrow checker
            {
                let query_type = self
                    .query_to_type
                    .get(field.node.name.node.as_str())
                    .unwrap();
                is_many = query_type.is_many;
                node_index = query_type.node_index;
            }

            query_string.push_str(
                &self.build_selection(selection_set.node.items.get(0).unwrap(), node_index),
            );
            if is_many {
                SQL::many_query(
                    &mut query_string,
                    &self.g[node_index].table_name,
                    &table_alias,
                );
            } else {
                match &field.node.arguments.get(0).unwrap().1.node {
                    Value::Number(num) => SQL::single_query(
                        &mut query_string,
                        &self.g[node_index].table_name,
                        num.as_i64().unwrap(),
                    ),
                    _ => println!("Didn't get Value::Number"),
                }
            }
        } else {
            panic!("First selection_set item isn't a field");
        }
        self.local_id += 1;
        query_string
    }

    fn build_selection(
        &mut self,
        selection: &Positioned<Selection>,
        node_index: NodeIndex<u32>,
    ) -> String {
        let mut to_return = String::new();
        if let Selection::Field(field) = &selection.node {
            //first we recursively get all queries from the children
            for selection in &field.node.selection_set.node.items {
                if let Selection::Field(child_field) = &selection.node {
                    //this field is terminal
                    let child_name = child_field.node.name.node.as_str();
                    if self.g[node_index].terminal_fields.contains(child_name) {
                        SQL::build_terminal_field(&mut to_return, child_name);
                    } else {
                        let mut edges = self
                            .g
                            .neighbors_directed(node_index, petgraph::EdgeDirection::Outgoing)
                            .detach();
                        to_return.push_str(
                            &self.build_foreign_field(selection, child_name, &mut edges, true),
                        );
                    }
                }
            }
        }
        to_return.drain(to_return.len() - 2..to_return.len());
        to_return
    }

    fn build_foreign_field(
        &mut self,
        selection: &Positioned<Selection>,
        parent_field_name: &str,
        parent_edges: &mut WalkNeighbors<u32>,
        include_to_json: bool,
    ) -> String {
        while let Some(edge) = parent_edges.next_edge(&self.g) {
            //found the edge which corresponds to this field
            if self.g[edge].graphql_field_name == parent_field_name {
                let mut to_return = String::new();

                //endpoints is a tuple where endpoints.0 contains the parent nodeindex, and
                //endpoints.1 contains the current graphql types node index
                let endpoints = self.g.edge_endpoints(edge).unwrap();

                //the start and end of this query, as well as the local_ids are different depending
                //on if its one to many or many to one. Everything in the middle is the same so
                //these arent the same methods
                self.local_id = SQL::join_head(
                    &mut to_return,
                    self.local_id,
                    include_to_json,
                    self.g[edge].one_to_many,
                );

                //we need a copy of this, as any further recursive calls would increment local_id
                //leading to incorrect results
                let local_id_copy = self.local_id;

                if let Selection::Field(field) = &selection.node {
                    for selection in &field.node.selection_set.node.items {
                        if let Selection::Field(child_field) = &selection.node {
                            let child_name = child_field.node.name.node.as_str();
                            //check if the child name is a terminal field
                            if !self.g[endpoints.1].terminal_fields.contains(child_name) {
                                //if not construct a nested join by adding the header, and passing
                                //the edges of this node to the child
                                SQL::nested_join_header(&mut to_return, child_name);
                                let mut edges = self
                                    .g
                                    .neighbors_directed(
                                        endpoints.1,
                                        petgraph::EdgeDirection::Outgoing,
                                    )
                                    .detach();
                                to_return.push_str(
                                    &self.build_foreign_field(
                                        selection, child_name, &mut edges, false,
                                    ),
                                );
                            } else {
                                SQL::build_terminal_field_join(
                                    &mut to_return,
                                    child_name,
                                    self.local_id,
                                );
                            }
                        }
                    }
                }
                SQL::join_tail(
                    &mut to_return,
                    local_id_copy,
                    include_to_json,
                    &self.g[endpoints.1].table_name,
                    &self.g[edge].foreign_key_name,
                    parent_field_name,
                    self.g[edge].one_to_many,
                );
                return to_return;
            }
        }
        panic!("{} endpoint not found found", parent_field_name)
    }
}
