use crate::{application, contracts::AppInfo};

#[tauri::command]
fn get_app_info() -> AppInfo {
    application::app_info().into()
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_info])
        .run(tauri::generate_context!())
        .expect("failed to run Wyss Console");
}

#[cfg(test)]
mod tests {
    use super::get_app_info;

    #[test]
    fn command_serializes_exact_app_info_v1_contract() {
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/app-info-v1.json")).unwrap();
        let actual = serde_json::to_value(get_app_info()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual["version"], env!("CARGO_PKG_VERSION"));
    }
}
