use serde::Deserialize;
use std::process::Command;

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
        .get("source/blender/nodes/CMakeFiles/bf_nodes.dir/intern/socket_search_link.cc.o")
        .expect("Didn't find command");

    let mut args = shlex::split(&command.command).expect("Failed to split command");
    let output_index = args.iter().position(|x| x == "-o").expect("Can't find -o");
    let mut assembly_file_path =
        std::path::Path::new(&command.directory).join(&args[output_index + 1]);
    assembly_file_path.set_extension("txt");
    args[output_index + 1] = assembly_file_path.to_str().unwrap().to_owned();
    args.insert(output_index, "-S".to_owned());

    println!("{:?}", args);

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
    println!("{:?}", assembly_file_path);
    std::fs::remove_file(assembly_file_path).expect("Can't remove file");
}
