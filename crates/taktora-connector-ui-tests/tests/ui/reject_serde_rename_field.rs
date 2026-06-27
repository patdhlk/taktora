use serde::Serialize;
use taktora_connector_ui::ViewModel;

#[derive(Serialize, ViewModel)]
struct Bad {
    #[serde(rename = "renamed")]
    field_one: bool,
}

fn main() {}
