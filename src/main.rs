extern crate bimap;
extern crate chrono;
extern crate indicatif;
extern crate regex;
extern crate serde;
extern crate serde_json;

use chrono::*;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize, Serialize)]
enum PlayerState {
    Playing,
    Inactive,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Player {
    name: String,
    state: PlayerState,
    hitpoints: f32,
    last_damaged: Option<String>,
    last_spawn_time: Option<DateTime<FixedOffset>>,
    last_down_time: Option<DateTime<FixedOffset>>,
    players_killed_by: HashMap<String, u32>,
    players_killed: HashMap<String, u32>,
    classes_played: HashSet<String>,
    players_revived_by: HashMap<String, u32>,
    players_revived: HashMap<String, u32>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Game {
    map: String,
    players: HashMap<String, Player>,
    start_time: DateTime<FixedOffset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GameState {
    games: Vec<Game>, // Sorted by start_time, from earliest to latest.
    current_game_start_time: DateTime<FixedOffset>,
    last_timestamp: DateTime<FixedOffset>,
    player_names: Vec<(String, Option<String>)>,
}

fn seen_player_name(
    name: &String,
    names: &Vec<(String, Option<String>)>,
) -> Vec<(String, Option<String>)> {
    let mut res = None;
    for (left, right) in names {
        if left == name {
            match right {
                Some(_t) => res = Some(names.clone()),
                None => (),
            }
        }
    }

    match res {
        Some(t) => t,
        None => {
            let mut my_names = names.clone();
            my_names.push((String::from(name), None));
            my_names
        }
    }
}

fn lookup_player_name(
    name: &String,
    names: &Vec<(String, Option<String>)>,
) -> (String, Vec<(String, Option<String>)>) {
    let mut res = None;
    for (left, right) in names {
        match right {
            Some(realname) => {
                if name == realname {
                    res = Some(left);
                }
            }
            None => (),
        }
    }

    match res {
        Some(n) => (n.clone(), names.clone()),
        None => {
            let mut new_names: Vec<(String, Option<String>)> = Vec::new();
            let my_names = names.clone();
            let mut found_name = None;
            for (left, right) in my_names {
                // Compute the length of the names in counts of characters rather
                // than byte lengths of the strings.
                let t1: Vec<char> = name.chars().collect();
                let t2: Vec<char> = left.chars().collect();
                let name_len = t1.len();
                let left_len = t2.len();

                if left_len <= name_len {
                    let tag_len = name_len - left_len;
                    let s1: String = name.chars().take(name_len).skip(tag_len).collect();

                    if s1 == left {
                        new_names.push((left.clone(), Some(name.clone())));
                        found_name = Some(left.clone())
                    } else {
                        new_names.push((left, right));
                    }
                } else {
                    new_names.push((left, right));
                }
            }

            (found_name.unwrap(), new_names)
        }
    }
}

fn game_ended(timestamp: &DateTime<FixedOffset>, g: &Game) {
    println!("ending at {}, Game: {:?}", timestamp, g);
}

fn get_dt(s: &str) -> Option<DateTime<FixedOffset>> {
    let s1 = format!("{} {}", s, "+0000").to_owned();
    let q = DateTime::parse_from_str(&s1[..], "%Y.%m.%d-%H.%M.%S:%3f %z");
    match q {
        Ok(v) => Some(v),
        Err(e) => {
            println!("{}", e);
            None
        }
    }
}

// Game state helper routines.

// The current game is the one that started at the time indicated by current_game_start_time.
fn get_current_game_idx(g: &GameState) -> usize {
    g.games
        .binary_search_by_key(&g.current_game_start_time, |t| t.start_time)
        .expect("Could not find game")
}

// Game state updating routines.

// Update that one player revived another.
fn player_revived(
    _timestamp: &DateTime<FixedOffset>,
    reviving: &str,
    revived: &str,
    g: &GameState,
) -> GameState {
    let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");

    // Find both players.
    let reviver_found = current_game.players.get(&String::from(reviving));
    let revivee_found = current_game.players.get(&String::from(revived));

    let f = match (reviver_found, revivee_found) {
        (Some(reviver), Some(revivee)) => {
            let mut players_revived = reviver.players_revived.clone();
            let mut players_revived_by = revivee.players_revived_by.clone();
            *players_revived.entry(String::from(revived)).or_insert(0) += 1;
            *players_revived_by.entry(String::from(revived)).or_insert(0) += 1;

            let new_reviver = Player {
                players_revived: players_revived,
                ..reviver.clone()
            };
            let new_revivee = Player {
                players_revived_by: players_revived_by,
                hitpoints: 5.0,
                ..revivee.clone()
            };
            Some((new_reviver, new_revivee))
        }
        _ => None,
    };

    let new_games = match f {
        Some((x, y)) => {
            let mut t1 = current_game.players.clone();
            *t1.get_mut(&x.name).unwrap() = x.clone();
            let mut t2 = t1.clone();
            *t2.get_mut(&y.name).unwrap() = y.clone();
            my_games.clone()
        }
        None => my_games.clone(),
    };

    GameState {
        games: new_games,
        current_game_start_time: g.current_game_start_time,
        last_timestamp: g.last_timestamp,
        player_names: g.player_names.clone(),
    }
}

// Add a player to the game state.
fn player_spawned(
    timestamp: &DateTime<FixedOffset>,
    name: &str,
    class: &str,
    g: &GameState,
) -> GameState {
    let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");

    // See if the player is in the current_game player hash set.
    let mut classes_played = HashSet::new();
    classes_played.insert(String::from(class));

    let new_player = match current_game.players.get(&String::from(name)) {
        Some(player) => {
            let a: HashSet<String> = classes_played.iter().cloned().collect();
            let b: HashSet<String> = player.classes_played.iter().cloned().collect();
            let c: HashSet<String> = a.union(&b).cloned().collect();
            // A player existed, update what classes they have played and their last
            // spawn time
            Player {
                state: PlayerState::Playing,
                classes_played: c,
                hitpoints: 100.0,
                last_damaged: None,
                last_spawn_time: Some(timestamp.clone()),
                ..player.clone()
            }
        }
        None => Player {
            name: String::from(name),
            state: PlayerState::Inactive,
            classes_played: classes_played.clone(),
            hitpoints: 100.0,
            last_damaged: None,
            last_down_time: None,
            last_spawn_time: Some(timestamp.clone()),
            players_killed: HashMap::new(),
            players_killed_by: HashMap::new(),
            players_revived: HashMap::new(),
            players_revived_by: HashMap::new(),
        },
    };

    current_game
        .players
        .insert(new_player.clone().name, new_player.clone());
    GameState {
        games: my_games,
        current_game_start_time: g.current_game_start_time,
        ..g.clone()
    }
}

// Called when a new map is loaded.
fn starting_game(timestamp: &DateTime<FixedOffset>, map_name: &str, g: &GameState) -> GameState {
    // Make a new Game.
    let new_game = Game {
        map: String::from(map_name),
        players: HashMap::new(),
        start_time: timestamp.clone(),
    };
    let mut games = g.games.clone();
    games.push(new_game);

    // Return a new GameState with our new game in it.
    GameState {
        games: games,
        current_game_start_time: timestamp.clone(),
        ..g.clone()
    }
}

fn player_damaged(
    _timestamp: &DateTime<FixedOffset>,
    shooter: &str,
    damage: f32,
    target: &str,
    _weapon: &str,
    g: &GameState,
) -> GameState {
    let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");
    let (resolved_name, new_player_names) =
        lookup_player_name(&String::from(target), &g.player_names);

    let retrieved_player = current_game
        .players
        .get(&resolved_name)
        .expect("Should have a player if they are shot");

    // If we know who did the damage, mark that in the player state for the player
    // that was shot.
    let new_shooter = if shooter != "nullptr" {
        Some(String::from(shooter))
    } else {
        retrieved_player.last_damaged.clone()
    };

    let updated_player = Player {
        last_damaged: new_shooter,
        hitpoints: retrieved_player.hitpoints - damage,
        ..retrieved_player.clone()
    };

    *current_game.players.get_mut(&updated_player.name).unwrap() = updated_player.clone();
    GameState {
        games: my_games,
        player_names: new_player_names,
        ..*g
    }
}

fn player_down(timestamp: &DateTime<FixedOffset>, player: &str, g: &GameState) -> GameState {
    let mut my_games = g.games.clone();
    let game_idx = get_current_game_idx(&g);
    let current_game = my_games.get_mut(game_idx).expect("Invalid index for game");
    let (resolved_player_name, new_player_names) =
        lookup_player_name(&String::from(player), &g.player_names);

    let retrieved_player = current_game
        .players
        .get(&resolved_player_name)
        .expect("Should have a player if they go down");

    // Who was the player last shot by? Update their stats with that information.

    let mut m_player_names = new_player_names.clone();
    match &retrieved_player.last_damaged {
        Some(killer_name) => {
            let (resolved_killer_name, new_player_names_2) =
                lookup_player_name(&String::from(killer_name), &new_player_names.clone());
            m_player_names = new_player_names_2;

            let killing_player = current_game
                .players
                .get(&resolved_killer_name)
                .expect("Should have this");
            let mut downed_killed_by = retrieved_player.players_killed_by.clone();
            *downed_killed_by.entry(resolved_killer_name).or_insert(0) += 1;
            let mut killing_killed = killing_player.players_killed.clone();
            *killing_killed.entry(resolved_player_name).or_insert(0) += 1;

            let new_downed_player = Player {
                last_damaged: None,
                last_down_time: Some(*timestamp),
                players_killed_by: downed_killed_by,
                ..retrieved_player.clone()
            };

            let new_killing_player = Player {
                players_killed: killing_killed,
                ..killing_player.clone()
            };

            *current_game
                .players
                .get_mut(&new_downed_player.name)
                .unwrap() = new_downed_player.clone();
            *current_game
                .players
                .get_mut(&new_killing_player.name)
                .unwrap() = new_killing_player.clone();
        }
        None => (),
    };

    GameState {
        games: my_games,
        player_names: m_player_names,
        ..*g
    }
}

// Parse routines.

fn parse_logsquad(
    timestamp: &DateTime<FixedOffset>,
    msg: &str,
    g: &GameState,
) -> Option<GameState> {
    let revive = Regex::new(r"(.*) has revived (.*)\.$").unwrap();

    let g1 = match revive.captures(msg) {
        Some(x) => Some(player_revived(timestamp, &x[1], &x[2], g)),
        None => None,
    };

    let damaged =
        Regex::new(r"Player:(.*) ActualDamage=(\d+\.\d+) from (.*) caused by (.*)$").unwrap();

    let g2 = match damaged.captures(msg) {
        Some(x) => {
            // Sometimes, someone damages nullptr. Ignore that.
            if &x[1] == "nullptr" {
                None
            } else {
                match g1 {
                    Some(t) => Some(player_damaged(
                        timestamp,
                        &x[3],
                        f32::from_str(&x[2]).unwrap(),
                        &x[1],
                        &x[4],
                        &t,
                    )),
                    None => Some(player_damaged(
                        timestamp,
                        &x[3],
                        f32::from_str(&x[2]).unwrap(),
                        &x[1],
                        &x[4],
                        g,
                    )),
                }
            }
        }
        None => match g1 {
            Some(x) => Some(x),
            None => None,
        },
    };

    g2
}

fn parse_logtrace(
    timestamp: &DateTime<FixedOffset>,
    msg: &str,
    g: &GameState,
) -> Option<GameState> {
    let role = Regex::new(r"\[DedicatedServer\]ASQPlayerController::SetCurrentRole\(\): On Server PC=(.*) NewRole=(.*)").unwrap();

    let g1 = match role.captures(msg) {
        Some(c) => {
            if &c[2] != "nullptr" {
                Some(player_spawned(timestamp, &c[1], &c[2], g))
            } else {
                None
            }
        }
        None => None,
    };

    let down = Regex::new(r"\[DedicatedServer\]ASQSoldier::Wound\(\): Player:(.*) KillingDamage=(\d+.\d+) from (.*) caused by (.*)").unwrap();

    let g2 = match down.captures(msg) {
        Some(c) => {
            if &c[1] == "nullptr" {
                None
            } else {
                match g1 {
                    Some(t) => Some(player_down(timestamp, &c[1], &t)),
                    None => Some(player_down(timestamp, &c[1], g)),
                }
            }
        }
        None => match g1 {
            Some(t) => Some(t),
            None => None,
        },
    };

    let statechange = Regex::new(r"\[DedicatedServer\]ASQPlayerController::ChangeState\(\): PC=(.*) OldState=(.*) NewState=(.*)").unwrap();

    let g3 = match statechange.captures(msg) {
        Some(c) => match g2 {
            Some(t) => {
                let newg = GameState {
                    player_names: seen_player_name(&String::from(&c[1]), &t.player_names),
                    ..t.clone()
                };
                Some(newg)
            }
            None => {
                let newg = GameState {
                    player_names: seen_player_name(&String::from(&c[1]), &g.player_names),
                    ..g.clone()
                };
                Some(newg)
            }
        },
        None => match g2 {
            Some(t) => Some(t),
            None => None,
        },
    };

    g3
}

fn parse_game_state(
    timestamp: &DateTime<FixedOffset>,
    msg: &str,
    g: &GameState,
) -> Option<GameState> {
    let game_state_change = Regex::new(r"Match State Changed from (\w+) to (\w+)$").unwrap();

    match game_state_change.captures(msg) {
        Some(x) => {
            match &x[2] {
                "WaitingPostMatch" => {
                    let mut my_games = g.games.clone();
                    if my_games.len() > 0 {
                        let game_idx = get_current_game_idx(&g);
                        let current_game =
                            my_games.get_mut(game_idx).expect("Invalid index for game");
                        game_ended(timestamp, current_game);
                        Some(GameState {
                            player_names: Vec::new(),
                            ..g.clone()
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            }
            /*if &x[2] == "WaitingPostMatch" {
                None
            } */
        }
        None => None,
    }
}

fn parse_world_state(
    timestamp: &DateTime<FixedOffset>,
    msg: &str,
    g: &GameState,
) -> Option<GameState> {
    let world_state_change = Regex::new(r"StartLoadingDestination to: /Game/Maps/(.*)").unwrap();

    let g1 = match world_state_change.captures(msg) {
        Some(x) => Some(starting_game(timestamp, &x[1], g)),
        None => None,
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
                // TODO: this throws away timestamps sometimes.
                let cur_g = GameState {
                    last_timestamp: timestamp,
                    ..g.clone()
                };

                match &c[2] {
                    "LogSquad" => parse_logsquad(&timestamp, &c[3], &cur_g),
                    "LogSquadTrace" => parse_logtrace(&timestamp, &c[3], &cur_g),
                    "LogGameState" => parse_game_state(&timestamp, &c[3], &cur_g),
                    "LogWorld" => parse_world_state(&timestamp, &c[3], &cur_g),
                    _ => Some(cur_g),
                }
            }
        }
        None => None,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerOutput {
    name: String,
    kills: HashMap<String, u32>,
    killed_by: HashMap<String, u32>,
    revives: HashMap<String, u32>,
    revived_by: HashMap<String, u32>,
    classes: HashSet<String>
}

fn print_lifetime_stats(g: &GameState) {
    let mut lifetime_players : HashMap<String, PlayerOutput> = HashMap::new();

    for game in &g.games {
        for (player_name, player_state) in &game.players {
            let updt = match lifetime_players.get(player_name) {
                Some(p) => {
                    // Merge everything. 
                    let mut new_kills = p.kills.clone(); 
                    for (n, c) in &player_state.players_killed {
                        *new_kills.entry(n.clone()).or_insert(0) += c;
                    }
                    let mut new_kills_by = p.killed_by.clone();
                    for (n,c) in &player_state.players_killed_by {
                        *new_kills_by.entry(n.clone()).or_insert(0) += c;
                    }
                    let mut new_revives = p.revives.clone();
                    for (n,c) in &player_state.players_revived {
                        *new_revives.entry(n.clone()).or_insert(0) += c;
                    }
                    let mut new_revived_by = p.revived_by.clone();
                    for (n,c) in &player_state.players_revived_by {
                        *new_revived_by.entry(n.clone()).or_insert(0) += c;
                    }

                    PlayerOutput {
                        kills: new_kills,
                        killed_by: new_kills_by,
                        revives: new_revives,
                        revived_by : new_revived_by,
                        .. p.clone()
                    }
                }
                None => PlayerOutput {
                    name: player_name.clone(),
                    kills: player_state.players_killed.clone(),
                    killed_by: player_state.players_killed_by.clone(),
                    revives: player_state.players_revived.clone(),
                    revived_by: player_state.players_revived_by.clone(),
                    classes: player_state.classes_played.clone()
                }
            };
            *lifetime_players.get_mut(player_name).unwrap() = updt.clone();
        }
    }

    println!("{}", serde_json::to_string(&g).expect("serialization error"));
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        panic!("expected statefile logfile");
    }

    let statefile = &args[1];
    let logfile = &args[2];

    let mut g = match fs::read_to_string(statefile) {
        Ok(statefile_lines) => serde_json::from_str::<GameState>(&statefile_lines).unwrap(),
        Err(_e) => GameState {
            games: Vec::new(),
            current_game_start_time: get_dt("1985.09.21-05.00.00:000").unwrap(),
            last_timestamp: get_dt("1985.09.21-05.00.00:000").unwrap(),
            player_names: Vec::new(),
        },
    };

    let logfile_contents = fs::read_to_string(logfile).expect("Error opening log file");
    let lines: Vec<&str> = logfile_contents.split("\n").collect();

    let pb = ProgressBar::new(logfile_contents.len() as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .progress_chars("#>-"));

    let mut new: u64 = 0;
    for line in &lines {
        new = new + (line.len() as u64);
        match parse_line(line, &g) {
            Some(new_g) => g = new_g,
            None => (),
        }
        pb.set_position(new);
    }

    print_lifetime_stats(&g);
    fs::write(statefile, serde_json::to_string(&g).expect("serialization error")).expect("IO");
}
