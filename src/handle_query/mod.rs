#[cfg(test)]
#[path = "./test.rs"]
mod test;
use crate::internal_schema_info::{GraphQLEdgeInfo, GraphQLType, QueryEdgeInfo};
use convert_case::{Case, Casing};
use graphql_parser::query::{
    parse_query, Definition, OperationDefinition, ParseError, Query, Selection,
};
use petgraph::graph::DiGraph;
use petgraph::prelude::NodeIndex;
use std::collections::HashMap;

pub struct Poggers {
    g: DiGraph<GraphQLType, GraphQLEdgeInfo>,
    query_to_type: HashMap<String, QueryEdgeInfo>,
    local_id: u8,
}

#[allow(dead_code)]
impl<'b> Poggers {
    pub fn build_root(&mut self, query: &str) -> Result<String, ParseError> {
        let ast = parse_query::<&str>(query)?;
        let definition = ast.definitions.get(0).unwrap();
        match definition {
            Definition::Operation(operation_definition) => {
                Ok(self.build_operation_definition(operation_definition))
            }
            Definition::Fragment(_fragment_definition) => {
                Ok(String::from("Definition::Fragment not implemented yet"))
            }
        }
    }

    fn build_operation_definition<'a>(
        &mut self,
        operation_definition: &'a OperationDefinition<&'a str>,
    ) -> String {
        match operation_definition {
            OperationDefinition::Query(query) => self.build_query(query),
            OperationDefinition::Subscription(_) => {
                String::from("Subscription not yet implemented")
            }

            OperationDefinition::Mutation(_) => String::from("Mutation not yet implemented"),

            OperationDefinition::SelectionSet(_) => {
                String::from("SelectionSet not yet implemented")
            }
        }
    }

    fn build_query<'a>(&mut self, query: &'a Query<&'a str>) -> String {
        let mut query_string = String::from(
            "select to_json(
              json_build_array(__local_0__.\"id\")
            ) as \"__identifiers\",
        ",
        );

        //create a __local__ string that we can use to distinguish this selection, and increment
        //the local_id to ensure that this stays as unique
        let mut local_string = String::from("__local_");
        local_string.push_str(&self.local_id.to_string());
        local_string.push_str("__");

        if let Selection::Field(field) = &query.selection_set.items[0] {
            let query_type = self.query_to_type.get(field.name).unwrap();
            query_string.push_str(
                &self.build_selection(&query.selection_set.items[0], query_type.node_index),
            );
            if query_type.is_many {
                query_string.push_str(" from ( select ");
                query_string.push_str(&local_string);
                query_string.push_str(".* from \"public\".\"");

                query_string.push_str(&self.g[query_type.node_index].table_name);
                query_string.push_str("\" as  ");
                query_string.push_str(&local_string);
                query_string.push_str(" order by ");
                query_string.push_str(&local_string);
                query_string.push_str(".\"id\" ASC ) ");
                query_string.push_str(&local_string);
            } else {
                query_string.push_str(" from \"public\".\"");
                query_string.push_str(&self.g[query_type.node_index].table_name);
                query_string.push_str("\" as __local_0__ where ( __local_0__.\"id\" = ");
                if let graphql_parser::schema::Value::Int(id) = &field.arguments.get(0).unwrap().1 {
                    query_string.push_str(&id.as_i64().unwrap().to_string());
                }
                query_string.push_str("\n )");
            }
        } else {
            panic!("First selection_set item isn't a field");
        }
        self.local_id += 1;
        query_string
    }

    fn build_selection<'a>(
        &self,
        selection: &'a Selection<&'a str>,
        node_index: NodeIndex<u32>,
    ) -> String {
        match selection {
            Selection::Field(field) => {
                let gql_type = &self.g[node_index];
                //first we recursively get all queries from the children
                let mut to_return = String::new();
                for selection in &field.selection_set.items {
                    match selection {
                        Selection::Field(child_field) => {
                            //this field is terminal
                            if gql_type.terminal_fields.contains(child_field.name) {
                                Poggers::build_terminal_field(&mut to_return, child_field.name);
                            } else {
                                let mut edges = self
                                    .g
                                    .neighbors_directed(
                                        node_index,
                                        petgraph::EdgeDirection::Outgoing,
                                    )
                                    .detach();
                                while let Some(edge) = edges.next_edge(&self.g) {
                                    //found the edge which corresponds to this field
                                    if self.g[edge].graphql_field_name == child_field.name {
                                        let endpoints = self.g.edge_endpoints(edge);
                                        match endpoints {
                                            Some(endpoints) => {
                                                to_return.push_str(&self.build_foreign_field(
                                                    selection,
                                                    endpoints.1,
                                                    &self.g[edge].foreign_key_name,
                                                    child_field.name,
                                                ));
                                                break;
                                            }
                                            None => panic!("No endpoints found"),
                                        }
                                    }
                                }
                            }
                        }
                        _ => panic!("Non field selection"),
                    }
                }

                to_return.pop();
                to_return
            }
            Selection::FragmentSpread(_) => String::from("FragmentSpread not implemented"),
            Selection::InlineFragment(_) => String::from("InlineFragment not implemented"),
        }
    }
    fn build_terminal_field(to_return: &mut String, field_name: &str) {
        to_return.push_str("to_json((__local_0__.\"");
        to_return.push_str(&field_name.to_case(Case::Snake));
        to_return.push_str("\")) as \"");
        to_return.push_str(field_name);
        to_return.push_str("\",");
    }

    fn build_foreign_field<'a>(
        &self,
        selection: &'a Selection<&'a str>,
        node_index: NodeIndex<u32>,
        foreign_key_name: &str,
        parent_field_name: &str,
    ) -> String {
        let mut to_return = String::from(
            "\nto_json(
                      (
                        select coalesce(
                          (
                            select json_agg(__local_",
        );

        to_return.push_str(&(self.local_id + 1).to_string());
        to_return.push_str(
            "__.\"object\")
                            from (
                              select json_build_object(
                                '__identifiers'::text,
                                json_build_array(__local_",
        );
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str("__.\"id\"), ");

        if let Selection::Field(field) = selection {
            for selection in field.selection_set.items.iter() {
                if let Selection::Field(child_field) = selection {
                    to_return.push('\'');
                    to_return.push_str(child_field.name);
                    to_return.push_str("'::text, (__local_");
                    to_return.push_str(&(self.local_id + 2).to_string());
                    to_return.push_str("__.\"");
                    to_return.push_str(&child_field.name.to_case(Case::Snake));
                    to_return.push_str("\")");
                }
            }
        }
        to_return.push_str(" ) as object ");
        to_return.push_str("from ( select __local_");
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str(
            "__.* 
                           from \"public\".\"",
        );

        let gql_type = &self.g[node_index];
        to_return.push_str(&gql_type.table_name);
        to_return.push_str("\" as __local_");
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str(
            "__
                                where (__local_",
        );
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str("__.\"");
        to_return.push_str(foreign_key_name);
        to_return.push_str("\" = __local_");
        to_return.push_str(&(self.local_id).to_string());
        to_return.push_str("__.\"id\") order by __local_");
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str(
            "__.\"id\" ASC
                              ) __local_",
        );
        to_return.push_str(&(self.local_id + 2).to_string());
        to_return.push_str(
            "__
                            ) as __local_",
        );
        to_return.push_str(&(self.local_id + 1).to_string());
        to_return.push_str(
            "__ ),
                          '[]'::json
                        )
                      )
                    )",
        );
        to_return.push_str(" as \"@");
        to_return.push_str(&parent_field_name);
        to_return.push_str("\" from (
                                  select __local_0__.*
                                  from \"public\".\"",
        );
        to_return.push_str(&self.g[node_index].table_name);
        to_return.push_str("\" as __local_");
        to_return.push_str(&(self.local_id).to_string());
        to_return.push_str(" where order by __local_");
        to_return.push_str(&(self.local_id).to_string());
        to_return.push_str(
            "__.\"id\" ASC
                                ) __local_",
        );
        to_return.push_str(&(self.local_id).to_string());
        to_return.push_str("__");
        to_return
    }
}
