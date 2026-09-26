use super::{ManagerResult, Package, PluginStore};
use serde_json::Value;
use std::io::{self, IsTerminal, Write};

pub fn configure(store: &PluginStore, name: &str, assignments: &[String]) -> ManagerResult<()> {
    let package = Package::load(&store.plugin_path(name)?)?;
    let mut options = store.options(name)?;
    let options = options.as_object_mut().ok_or("保存された設定が不正です")?;
    if assignments.is_empty() {
        if !io::stdin().is_terminal() {
            return Err("非対話実行では--set 項目=値を指定してください".into());
        }
        let fields = package.manifest.config_schema.get("properties").and_then(Value::as_object).ok_or("設定項目の定義がありません")?;
        for (name, definition) in fields {
            let previous = options.get(name).or_else(|| definition.get("default"));
            println!("{}: {}", name, definition.get("description").and_then(Value::as_str).unwrap_or(""));
            let secret = definition.get("writeOnly").and_then(Value::as_bool).unwrap_or(false);
            // シークレットは環境変数名を設定する設計にする。平文入力を端末へ表示しない。
            if secret {
                return Err("シークレットの直接入力は非対応です。環境変数を参照する設定を使ってください".into());
            }
            print!("値（空欄で{}）: ", previous.map_or("省略".to_owned(), |value| value.to_string()));
            io::stdout().flush()?;
            let mut line = String::new();
            if io::stdin().read_line(&mut line)? == 0 {
                return Err("入力が終了しました。設定は保存していません".into());
            }
            let line = line.trim();
            if !line.is_empty() {
                options.insert(name.clone(), parse_value(line, definition));
            } else if let Some(value) = previous.cloned() {
                options.insert(name.clone(), value);
            }
        }
    } else {
        apply_assignments(options, &package.manifest.config_schema, assignments)?;
    }
    store.save_options(name, &Value::Object(options.clone()))?;
    println!("{name}の設定を保存しました。次回の中継起動で反映されます。");
    Ok(())
}

fn parse_value(text: &str, definition: &Value) -> Value {
    if definition.get("type").and_then(Value::as_str) == Some("string") {
        Value::String(text.to_owned())
    } else {
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
    }
}

pub(crate) fn apply_assignments(options: &mut serde_json::Map<String, Value>, schema: &Value, assignments: &[String]) -> ManagerResult<()> {
    for assignment in assignments {
        let (name, value) = assignment.split_once('=').ok_or("--set 項目=値で指定してください")?;
        options.insert(name.to_owned(), parse_value(value, &schema["properties"][name]));
    }
    Ok(())
}
