extern crate regex;
extern crate chrono;
extern crate indicatif;

use std::env;
use std::cmp;
use std::fs;
use regex::Regex;
use chrono::*;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Debug,Clone)]
enum PlayerState {
    Playing,
    Inactive,
}

#[derive(Debug,Clone)]
struct Player {
    name: String,
    state: PlayerState,
    hitpoints: i32,
    last_damaged: Option<String>,
    last_spawn_time: Option<DateTime<FixedOffset>>,
    last_down_time: Option<DateTime<FixedOffset>>,
    players_killed_by: HashSet<String>,
    players_killed: HashSet<String>,
    classes_played: HashSet<String>,
    players_revived_by: HashSet<String>,
    players_revived: HashSet<String>,
}
impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for Player {}
impl Hash for Player {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[derive(Debug, Clone)]
struct Game {
    map: String,
    players: HashSet<Player>,
    start_time: DateTime<FixedOffset>,
}

#[derive(Debug, Clone)]
struct GameState {
    games: Vec<Game>, // Sorted by start_time, from earliest to latest. 
    current_game_start_time: DateTime<FixedOffset>,
    last_timestamp: DateTime<FixedOffset>,
}

fn game_ended(g: &Game) {
    println!("Game on map {} started at {} ended", g.map, g.start_time);
}

fn get_dt(s: &str) -> Option<DateTime<FixedOffset>> {
    let s1 = format!("{} {}", s, "+0000").to_owned();
    let q = DateTime::parse_from_str(&s1[..], "%Y.%m.%d-%H.%M.%S:%3f %z");
    match q {
        Ok(v) => Some(v),
        Err(e) => { println!("{}", e); 
                    None
        }
    }
}

// Game state helper routines.

// The current game is the one that started at the time indicated by current_game_start_time.
fn get_current_game_idx(g: &GameState) -> usize {
    g.games.binary_search_by_key(&g.current_game_start_time, |t| t.start_time).expect("Could not find game")
}

// Game state updating routines.

// Update that one player revived another. 
fn player_revived(timestamp: &DateTime<FixedOffset>, reviving: &str, revived: &str, g: GameState) -> GameState {
	let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");

	// Find both players. 

	GameState { games: my_games, 
				current_game_start_time : g.current_game_start_time, 
				last_timestamp: g.last_timestamp }
}

// Add a player to the game state. 
fn player_spawned(timestamp: &DateTime<FixedOffset>, name: &str, class: &str, g: &GameState) -> GameState {
	let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");
	
	// See if the player is in the current_game player hash set. 
	let mut classes_played = HashSet::new();
	classes_played.insert(String::from(class));
	let candidate_player = Player { name : String::from(name),
									state: PlayerState::Inactive,
									classes_played : classes_played.clone(),
									hitpoints : 100,
									last_damaged : None,
									last_down_time : None,
									last_spawn_time : Some(timestamp.clone()),
									players_killed : HashSet::new(),
									players_killed_by : HashSet::new(),
									players_revived : HashSet::new(),
									players_revived_by : HashSet::new() };

	let new_player = match current_game.players.get(&candidate_player) {
		Some(player) => {
			let a : HashSet<String> = classes_played.iter().cloned().collect();
			let b : HashSet<String> = player.classes_played.iter().cloned().collect();
			let c : HashSet<String> = a.union(&b).cloned().collect();
			// A player existed, update what classes they have played and their last 
			// spawn time
			Player {	name : player.name.clone(),
						state : PlayerState::Playing,
						classes_played : c,
						hitpoints : player.hitpoints,
						last_damaged : player.last_damaged.clone(),
						last_down_time : player.last_down_time,
						last_spawn_time : Some(timestamp.clone()),
						players_killed : player.players_killed.clone(),
						players_killed_by : player.players_killed_by.clone(),
						players_revived : player.players_revived.clone(),
						players_revived_by : player.players_revived_by.clone() }
		},
		None => {
			candidate_player
		}
	};

	current_game.players.replace(new_player);
    GameState { games: my_games, 
				current_game_start_time : g.current_game_start_time, 
				last_timestamp: g.last_timestamp }
}

// Called when a new map is loaded. 
fn starting_game(timestamp: &DateTime<FixedOffset>, map_name: &str, g: &GameState) -> GameState {
	// Make a new Game. 
	let new_game = Game { map: String::from(map_name), 
						players: HashSet::new(), 
						start_time : timestamp.clone() };
	let mut games = g.games.clone();
	games.push(new_game);

	// Return a new GameState with our new game in it. 
	GameState { games : games,
				last_timestamp : g.last_timestamp.clone(),
				current_game_start_time : timestamp.clone() }
}

// Parse routines.

fn parse_logsquad(timestamp: &DateTime<FixedOffset>, msg: &str, g: &GameState) -> Option<GameState> {
    let revive = Regex::new(r"(.*) has revived (.*)\.$").unwrap();

    match revive.captures(msg) {
        Some(x) => println!("At {}, {} revived {}", timestamp, &x[1], &x[2]),
        None => ()
    }

    let damaged = Regex::new(r"Player:(.*) ActualDamage=(\d+\.\d+) from (.*) caused by (.*)$").unwrap();

    match damaged.captures(msg) {
        Some(x) => println!("At {}, {} did {} damage to {} with {}", timestamp, &x[3], &x[2], &x[1], &x[4]),
        None => ()
    }

    None
}

fn parse_logtrace(timestamp: &DateTime<FixedOffset>, msg: &str, g: &GameState) -> Option<GameState> {
    let role = Regex::new(r"\[DedicatedServer\]ASQPlayerController::SetCurrentRole\(\): On Server PC=(.*) NewRole=(.*)").unwrap();

    let g1 = match role.captures(msg) {
        Some(c) => if &c[2] != "nullptr" { 
			println!("At {}, player {} classed {}", timestamp, &c[1], &c[2]);
			Some(player_spawned(timestamp, &c[1], &c[2], g))
		} else { None },
        None => None
    };

    let down = Regex::new(r"\[DedicatedServer\]ASQSoldier::Wound\(\): Player:(.*) KillingDamage=(\d+.\d+) from (.*) caused by (.*)").unwrap();

    match down.captures(msg) {
        Some(c) => println!("At {}, player {} went down", timestamp, &c[1]),
        None => ()
    }

    let statechange = Regex::new(r"\[DedicatedServer\]ASQPlayerController::ChangeState\(\): PC=(.*) OldState=(.*) NewState=(.*)").unwrap();

    match statechange.captures(msg) {
        Some(c) => println!("At {}, player {} changed from {} to {}", timestamp, &c[1], &c[2], &c[3]),
        None => ()
    }

	g1
}

fn parse_game_state(timestamp: &DateTime<FixedOffset>, msg: &str, g: &GameState) -> Option<GameState> {
    let game_state_change = Regex::new(r"Match State Changed from (\w+) to (\w+)$").unwrap();

    match game_state_change.captures(msg) {
        Some(x) => { 
            println!("At {} game state changed from {} to {}", timestamp, &x[1], &x[2]);
        },
        None => ()
    }

    None
}

fn parse_world_state(timestamp: &DateTime<FixedOffset>, msg: &str, g: &GameState) -> Option<GameState> {
    let world_state_change = Regex::new(r"StartLoadingDestination to: /Game/Maps/(.*)").unwrap();

    let g1 = match world_state_change.captures(msg) {
        Some(x) => {    
            println!("At {}, starting game {}", timestamp, &x[1]);
			Some(starting_game(timestamp, &x[1], g))
        },
        None => None
    };

	g1
}

fn parse_line(line: &str, g: &GameState) -> Option<GameState> {
    let logline_re = Regex::new(r"^\[(\d+.\d+.\d+-\d+.\d+.\d+:\d+)\]\[.*\](\w+): (.*)").unwrap();
    match logline_re.captures(line) {
        Some(c) => {
            // Update the timestamp if the current line is newer, even if we won't process this
            // line into an update to the game state. 
            let timestamp = get_dt(&c[1]).unwrap();
			if timestamp < g.last_timestamp {
				None
			} else {	
				let cur_g = GameState { games: g.games.clone(), 
								current_game_start_time : g.current_game_start_time.clone(), 
								last_timestamp: timestamp };

				match &c[2] {
					"LogSquad" => parse_logsquad(&timestamp, &c[3], &cur_g),
					"LogSquadTrace" => parse_logtrace(&timestamp, &c[3], &cur_g),
					"LogGameState" => parse_game_state(&timestamp, &c[3], &cur_g),
					"LogWorld" => parse_world_state(&timestamp, &c[3], &cur_g),
					_ => Some(cur_g),
            }}},
        None => None
    }
}

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

    let pb = ProgressBar::new(lines.len() as u64);  
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {lines}/{total_lines} ({eta})")
        .progress_chars("#>-"));

    let mut new :u64 = 0;
    let mut g = GameState { games: Vec::new(), current_game_start_time : get_dt("1985.09.21-05.00.00:000").unwrap(), last_timestamp: get_dt("1985.09.21-05.00.00:000").unwrap() };
    for line in &lines {
        new = new + 1;
        match parse_line(line, &g) {
            Some(new_g) => g = new_g,
            None => ()
        }
        pb.set_position(new);
    }
}
