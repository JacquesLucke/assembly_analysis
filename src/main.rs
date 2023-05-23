use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{io::Read, process::Command};

#[derive(Deserialize, Debug)]
struct CMakeCompileCommand {
    directory: String,
    command: String,
    file: String,
    output: String,
}

struct AssemblyGenerationCommand {
    program: PathBuf,
    args: Vec<String>,
    cwd: String,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct FunctionInfo<'a> {
    name: &'a str,
    callees: std::collections::HashSet<&'a str>,
    instructions_num: usize,
}

impl<'a> FunctionInfo<'a> {
    fn new(name: &'a str) -> FunctionInfo {
        FunctionInfo {
            name: name,
            callees: std::collections::HashSet::new(),
            instructions_num: 0,
        }
    }
}

fn load_cmake_compile_commands(path: &std::path::Path) -> Result<Vec<CMakeCompileCommand>> {
    let file = std::fs::File::open(path)?;
    let compile_commands: Vec<CMakeCompileCommand> = serde_json::from_reader(file)?;
    Ok(compile_commands)
}

fn adapt_cmake_command_to_generate_assembly(
    command: &CMakeCompileCommand,
) -> Result<AssemblyGenerationCommand> {
    let mut args =
        shlex::split(&command.command).ok_or(eyre::eyre!("Can't split cmake command."))?;
    let output_index = args
        .iter()
        .position(|x| x == "-o")
        .ok_or(eyre::eyre!("Can't find -o in the command."))?;
    let mut assembly_file_path =
        std::path::Path::new(&command.directory).join(&args[output_index + 1]);
    assembly_file_path.set_extension("txt");
    args[output_index + 1] = assembly_file_path
        .to_str()
        .ok_or(eyre::eyre!("Failed to create assembly output path."))?
        .to_owned();
    args.insert(output_index, "-S".to_owned());
    Ok(AssemblyGenerationCommand {
        program: std::path::PathBuf::from(args[0].clone()),
        args: args[1..].to_owned(),
        cwd: command.directory.clone(),
        output: assembly_file_path,
    })
}

fn run_assembly_generation(command: &AssemblyGenerationCommand) -> Result<()> {
    let mut process = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .spawn()?;
    if !process.wait()?.success() {
        return Err(eyre::eyre!("Generating assembly failed."));
    }
    Ok(())
}

fn get_assembly_of_cmake_command(cmake_command: &CMakeCompileCommand) -> Result<String> {
    let asm_command = adapt_cmake_command_to_generate_assembly(cmake_command)?;
    run_assembly_generation(&asm_command)?;

    let assembly = std::fs::read_to_string(&asm_command.output)?;
    std::fs::remove_file(&asm_command.output).expect("Can't remove file");

    Ok(assembly)
}

#[derive(Debug, Serialize)]
struct ObjectFileAssemblyInfo<'a> {
    info_by_function: HashMap<&'a str, FunctionInfo<'a>>,
}

fn parse_assembly(assembly: &str) -> Result<ObjectFileAssemblyInfo> {
    let mut info = ObjectFileAssemblyInfo {
        info_by_function: HashMap::new(),
    };
    let mut current_symbol: Option<&str> = None;
    for line in assembly.lines() {
        if !line.starts_with("\t") && !line.starts_with(".") && line.len() >= 3 {
            let symbol = &line[1..line.len() - 1];
            current_symbol = Some(symbol);
            info.info_by_function
                .insert(symbol, FunctionInfo::new(symbol));
            continue;
        }
        match current_symbol {
            Some(symbol) => {
                let line = line.trim();
                let mut function_info = info.info_by_function.get_mut(symbol).unwrap();

                if line.starts_with("call") {
                    let mangled_name = line
                        .split_ascii_whitespace()
                        .nth(1)
                        .ok_or(eyre::eyre!("Couldn't parse function name."))?;
                    function_info.callees.insert(mangled_name);
                }
                if !line.starts_with(".") {
                    function_info.instructions_num += 1;
                }
            }
            None => {}
        }
    }

    info.info_by_function.retain(|_, x| x.instructions_num > 0);
    Ok(info)
}

fn app() -> Result<()> {
    let compile_commands_path =
        Path::new("/home/jacques/blender/build_debug/compile_commands.json");
    let compile_commands = load_cmake_compile_commands(compile_commands_path)?;

    let mut command_by_output = HashMap::new();
    for command in &compile_commands {
        command_by_output.insert(command.output.as_str(), command);
    }

    let command = command_by_output
        .get("source/blender/modifiers/CMakeFiles/bf_modifiers.dir/intern/MOD_volume_displace.cc.o")
        .ok_or(eyre::eyre!("Can't find compile command."))?;
    let assembly = get_assembly_of_cmake_command(command)?;

    let info = parse_assembly(&assembly)?;

    let output_json = serde_json::json!(info).to_string();
    std::fs::write("test.json", output_json)?;

    println!("{:#?}", info);
    Ok(())
}

fn main() {
    match app() {
        Ok(_) => {}
        Err(err) => {
            println!("{:?}", err);
        }
    }
}
