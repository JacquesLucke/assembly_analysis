use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Deserialize, Debug)]
struct CMakeCompileCommand {
    directory: String,
    command: String,
    output: String,
}

struct AssemblyGenerationCommand {
    program: PathBuf,
    args: Vec<String>,
    cwd: String,
    output: PathBuf,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq, Hash)]
struct FunctionID(usize);

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq, Hash)]
struct ObjectID(usize);

#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone)]
struct GlobalFunctionName {
    name: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone)]
struct LocalFunctionName {
    object: ObjectID,
    name: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone)]
enum FunctionName {
    Global(GlobalFunctionName),
    Local(LocalFunctionName),
}

#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone)]
struct ObjectName {
    path: PathBuf,
}

#[derive(Debug, Default, Serialize)]
struct ParsedData {
    object_id_by_name: HashMap<ObjectName, ObjectID>,
    name_by_object_id: HashMap<ObjectID, ObjectName>,

    function_id_by_name: HashMap<FunctionName, FunctionID>,
    name_by_function_id: HashMap<FunctionID, FunctionName>,

    functions_by_object: HashMap<ObjectID, Vec<FunctionID>>,
    objects_by_function: HashMap<FunctionID, Vec<ObjectID>>,

    callers_by_callee: HashMap<FunctionID, Vec<FunctionID>>,
    callees_by_caller: HashMap<FunctionID, Vec<FunctionID>>,

    instructions_by_function: HashMap<FunctionID, usize>,
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

enum LinkType {
    Local,
    Weak,
    Global,
}

fn parse_data(object: ObjectID, assembly: &str, parsed: &mut ParsedData) {
    let mut link_type_by_name: HashMap<&str, LinkType> = HashMap::new();
    let mut function_names: HashSet<&str> = HashSet::new();
    let mut aliases: HashMap<&str, &str> = HashMap::new();

    for line in assembly.lines() {
        let trimmed_line = line.trim();
        if trimmed_line.starts_with(".type") && trimmed_line.ends_with("@function") {
            let function_name =
                &trimmed_line[".type\t".len()..(trimmed_line.len() - ", @function".len())];
            function_names.insert(function_name);
        } else if trimmed_line.starts_with(".weak\t") {
            let function_name = &trimmed_line[".weak\t".len()..];
            link_type_by_name.insert(function_name, LinkType::Weak);
        } else if trimmed_line.starts_with(".globl\t") {
            let function_name = &trimmed_line[".globl\t".len()..];
            link_type_by_name.insert(function_name, LinkType::Global);
        } else if trimmed_line.starts_with(".set\t") {
            if let Some(comma_i) = trimmed_line.find(",") {
                let old_name = &trimmed_line[".set\t".len()..comma_i];
                let new_name = &trimmed_line[comma_i..];
                aliases.insert(old_name, new_name);
            }
        }
    }

    let mut id_by_function_name: HashMap<&str, FunctionID> = HashMap::new();

    for &function_name in function_names.iter() {
        let link_type = link_type_by_name
            .get(function_name)
            .unwrap_or(&LinkType::Local);
        let function = match link_type {
            LinkType::Local => FunctionName::Local(LocalFunctionName {
                object: object,
                name: function_name.to_owned(),
            }),
            _ => FunctionName::Global(GlobalFunctionName {
                name: function_name.to_owned(),
            }),
        };
        let next_function_id = FunctionID(parsed.function_id_by_name.len());
        let function_id = *parsed
            .function_id_by_name
            .entry(function.clone())
            .or_insert(next_function_id);
        parsed.name_by_function_id.insert(function_id, function);

        id_by_function_name.insert(function_name, function_id);
    }

    let mut current_function: Option<FunctionID> = None;
    for line in assembly.lines() {
        if let Some(function_id) = current_function {
            let trimmed_line = line.trim();
            if trimmed_line.starts_with(".size\t") {
                current_function = None;
                continue;
            }
            if trimmed_line.starts_with(".") {
                continue;
            }
            *parsed
                .instructions_by_function
                .entry(function_id)
                .or_default() += 1;
            if trimmed_line.starts_with("call\t") {
                let mut callee = &trimmed_line["call\t".len()..];
                if !callee.starts_with("*%") {
                    if callee.ends_with("@PLT") {
                        callee = &callee[..(callee.len() - "@PLT".len())];
                    }
                    if let Some(alias) = aliases.get(callee) {
                        callee = alias;
                    }
                    let callee_id = if let Some(callee_id) = id_by_function_name.get(callee) {
                        *callee_id
                    } else {
                        let next_function_id = FunctionID(parsed.function_id_by_name.len());
                        let callee_name = FunctionName::Global(GlobalFunctionName {
                            name: callee.to_owned(),
                        });
                        let callee_id = *parsed
                            .function_id_by_name
                            .entry(callee_name.clone())
                            .or_insert(next_function_id);
                        parsed
                            .name_by_function_id
                            .insert(next_function_id, callee_name);
                        callee_id
                    };
                    parsed
                        .callees_by_caller
                        .entry(function_id)
                        .or_default()
                        .push(callee_id);
                    parsed
                        .callers_by_callee
                        .entry(callee_id)
                        .or_default()
                        .push(function_id);
                }
            }
        } else {
            if line.starts_with("\t") {
                continue;
            }
            if !line.ends_with(":") {
                continue;
            }
            let label_name = &line[..line.len() - 1];
            if let Some(function_id) = id_by_function_name.get(label_name).copied() {
                current_function = Some(function_id);
                parsed
                    .functions_by_object
                    .entry(object)
                    .or_default()
                    .push(function_id);
                parsed
                    .objects_by_function
                    .entry(function_id)
                    .or_default()
                    .push(object);
            }
        }
    }
}

fn print_functions_with_most_instructions(parsed: &ParsedData) {
    let mut data: Vec<_> = parsed.instructions_by_function.iter().collect();
    data.sort_by(|a, b| a.1.cmp(b.1).reverse());
    for (function_id, instr_num) in data {
        let function = parsed.name_by_function_id.get(function_id).unwrap();
        println!("{:?}: {}", function, instr_num);
    }
}

fn print_functions_in_all_objects(parsed: &ParsedData) {
    let objects_num = parsed.object_id_by_name.len();
    for (function_id, objects) in parsed.objects_by_function.iter() {
        if objects.len() == objects_num {
            let function = parsed.name_by_function_id.get(function_id).unwrap();
            println!("{:?}", function);
        }
    }
}

fn print_function_info(parsed: &ParsedData, function: &FunctionName) -> Result<()> {
    let function_id = parsed
        .function_id_by_name
        .get(function)
        .ok_or(eyre::eyre!("Can't find function."))?;
    let objects = parsed
        .objects_by_function
        .get(function_id)
        .cloned()
        .unwrap_or_default();
    let callers = parsed
        .callers_by_callee
        .get(function_id)
        .cloned()
        .unwrap_or_default();
    let callees = parsed
        .callees_by_caller
        .get(function_id)
        .cloned()
        .unwrap_or_default();
    println!("Function: {:?}", function);
    println!("  Objects:");
    for object in objects {
        println!("    {:?}", parsed.name_by_object_id.get(&object).unwrap());
    }
    println!("  Callers:");
    for caller in callers {
        println!("    {:?}", parsed.name_by_function_id.get(&caller).unwrap());
    }
    println!("  Callees:");
    for callee in callees {
        println!("    {:?}", parsed.name_by_function_id.get(&callee).unwrap());
    }
    Ok(())
}

fn app() -> Result<()> {
    let compile_commands_path =
        Path::new("/home/jacques/blender/build_debug/compile_commands.json");
    let compile_commands = load_cmake_compile_commands(compile_commands_path)?;

    let mut command_by_output = HashMap::new();
    for command in &compile_commands {
        command_by_output.insert(command.output.as_str(), command);
    }

    let files = vec![
        "source/blender/functions/CMakeFiles/bf_functions.dir/intern/field.cc.o",
        "source/blender/functions/CMakeFiles/bf_functions.dir/intern/lazy_function_graph_executor.cc.o",
    ];

    let mut parsed = ParsedData::default();

    for file in files {
        let command = command_by_output
            .get(file)
            .ok_or(eyre::eyre!("Can't find compile command."))?;
        let now = std::time::Instant::now();
        let assembly = get_assembly_of_cmake_command(command)?;
        println!("Generate Assembly: {} ms", now.elapsed().as_millis());

        let next_object_id = ObjectID(parsed.object_id_by_name.len());
        let object_name = ObjectName { path: file.into() };
        let object = *parsed
            .object_id_by_name
            .entry(object_name.clone())
            .or_insert(next_object_id);
        parsed
            .name_by_object_id
            .entry(object)
            .or_insert(object_name);

        let now = std::time::Instant::now();
        parse_data(object, &assembly, &mut parsed);
        println!("Parse: {} ms", now.elapsed().as_millis());
    }

    // print_functions_with_most_instructions(&parsed);
    // print_functions_in_all_objects(&parsed);
    print_function_info(
        &parsed,
        &FunctionName::Global(GlobalFunctionName {
            name: "_ZN7blender10IndexRangeC2El".to_owned(),
        }),
    )?;

    // let output_json = serde_json::json!(info).to_string();
    // std::fs::write("test.json", output_json)?;

    // println!("{:#?}", info);
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
