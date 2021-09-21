use convert_case::{Case, Casing};
use graphql_parser::query::{
    parse_query, Definition, OperationDefinition, ParseError, Query, Selection,
};
use std::collections::HashMap;

struct SqlOperation {
    table_name: String,
    is_many: bool,
}
pub struct Poggers<'a> {
    pub graphql_query_to_operation: HashMap<&'a str, SqlOperation>,
}

impl Poggers<'_> {
    pub fn build_root(query: &str) -> Result<String, ParseError> {
        let ast = parse_query::<&str>(query)?;
        let definition = ast.definitions.iter().next().unwrap();
        match definition {
            Definition::Operation(operation_definition) => {
                return Ok(Poggers::build_operation_definition(operation_definition));
            }
            Definition::Fragment(_fragment_definition) => {
                return Ok(String::from("Definition::Fragment not implemented yet"));
            }
        }
    }

    fn build_operation_definition<'a>(
        operation_definition: &'a OperationDefinition<&'a str>,
    ) -> String {
        match operation_definition {
            OperationDefinition::Query(query) => Poggers::build_query(query),
            OperationDefinition::Subscription(_) => {
                return String::from("Subscription not yet implemented");
            }
            OperationDefinition::Mutation(_) => {
                return String::from("Mutation not yet implemented");
            }
            OperationDefinition::SelectionSet(_) => {
                return String::from("SelectionSet not yet implemented");
            }
        }
    }

    fn build_query<'a>(query: &'a Query<&'a str>) -> String {
        let mut query_string = String::from(
            "select to_json(
          json_build_array(__local_0__.\"id\")
        ) as \"__identifiers\",
        ",
        );
        query_string.push_str(&Poggers::build_selection(&query.selection_set.items[0]));
        query_string
    }

    fn build_selection<'a>(&self, selection: &'a Selection<&'a str>) -> String {
        match selection {
            Selection::Field(field) => {
                //leaf node
                if field.selection_set.items.is_empty() {
                    //simply add json field with the field name in snake case
                    let mut to_return = String::from("to_json((__local_0__.\"");
                    to_return.push_str(&field.name.to_case(Case::Snake));
                    to_return.push_str("\")) as \"");
                    to_return.push_str("\")) as \"");
                    to_return.push_str("\",");
                    to_return
                } else {
                    //first we recursively get all queries from the children
                    let mut query_string = field
                        .selection_set
                        .items
                        .iter()
                        .map(|selection| Poggers::build_selection(selection))
                        .collect::<Vec<String>>()
                        .join("");

                    //the last select has an unnecessary comma which causes syntax errors
                    query_string.pop();

                    println!("{}", self.graphql_query_to_operation);
                    if let Some((name, val)) = field.arguments.iter().next() {
                        query_string.push_str("from \"public\".\"");
                        //query_string.push_str(&field.name.to_singular());
                        query_string.push_str("\" as __local_0__ ");
                        query_string.push_str("where ( __local_0__.\"");
                        query_string.push_str(name);
                        query_string.push_str("\" = ");
                        query_string.push_str(&val.to_string());
                        query_string.push(')');
                    } else {
                        //select all the child fields from this
                        //
                        query_string.push_str(
                            "from (
                              select __local_0__.*
                              from \"public\".\")",
                        );
                        //query_string.push_str(&field.name.to_singular());
                        query_string.push_str(
                            "\" as __local_0__
                              order by __local_0__.\"id\" ASC
                          )",
                        );
                    }
                    query_string
                }
            }
            Selection::FragmentSpread(_) => String::from("FragmentSpread not implemented"),
            Selection::InlineFragment(_) => String::from("InlineFragment not implemented"),
        }
    }
}
