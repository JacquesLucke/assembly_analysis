use serde::Deserialize;
use std::{collections::HashMap, io::Read, process::Command};

#[derive(Deserialize, Debug)]
struct CMakeCompileCommand {
    directory: String,
    command: String,
    file: String,
    output: String,
}

fn main() {
    let compile_commands_path = "/home/jacques/blender/build_debug/compile_commands.json";
    let file = std::fs::File::open(compile_commands_path).expect("Couldn't load compile commands");
    let compile_commands: Vec<CMakeCompileCommand> =
        serde_json::from_reader(file).expect("Couldn't parse file");

    let mut command_by_output = std::collections::hash_map::HashMap::new();
    for command in &compile_commands {
        command_by_output.insert(command.output.as_str(), command);
    }

    let command = command_by_output
        .get("source/blender/modifiers/CMakeFiles/bf_modifiers.dir/intern/MOD_uvwarp.cc.o")
        .expect("Didn't find command");

    let mut args = shlex::split(&command.command).expect("Failed to split command");
    let output_index = args.iter().position(|x| x == "-o").expect("Can't find -o");
    let mut assembly_file_path =
        std::path::Path::new(&command.directory).join(&args[output_index + 1]);
    assembly_file_path.set_extension("txt");
    args[output_index + 1] = assembly_file_path.to_str().unwrap().to_owned();
    args.insert(output_index, "-S".to_owned());

    let command = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(&command.directory)
        .spawn();
    match command {
        Ok(mut child) => {
            let exit_status = child.wait().expect("Failed");
            if exit_status.success() {
                println!("Process existed successfully");
            } else {
                println!("Error");
            }
        }
        Err(err) => {
            println!("Failed: {}", err);
        }
    }

    let mut assembly = String::new();
    std::fs::File::open(&assembly_file_path)
        .unwrap()
        .read_to_string(&mut assembly)
        .unwrap();

    let mut lines_by_symbol: HashMap<&str, Vec<&str>> = std::collections::hash_map::HashMap::new();
    let mut current_symbol: Option<&str> = None;
    for line in assembly.lines() {
        if !line.starts_with("\t") && !line.starts_with(".") && line.len() >= 3 {
            let symbol = &line[1..line.len() - 1];
            current_symbol = Some(symbol);
            continue;
        }
        match current_symbol {
            Some(symbol) => {
                let line = line.trim();
                if line.starts_with("call") {
                    let values = lines_by_symbol.entry(symbol).or_insert(Vec::new());
                    values.push(line);
                }
            }
            None => {}
        }
    }

    println!("{:#?}", lines_by_symbol)
    // std::fs::remove_file(assembly_file_path).expect("Can't remove file");
}
