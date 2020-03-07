use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        panic!("expected statefile logfile");
    }

    let statefile = &args[1];
    let logfile = &args[2];
    println!("statefile == {} logfile == {}", statefile, logfile);

    let logfile_contents = fs::read_to_string(logfile)
        .expect("Error opening log file");
    let lines: Vec<&str> = logfile_contents.split("\n").collect();

    for line in &lines {
        println!("{}", line);
    }
}
