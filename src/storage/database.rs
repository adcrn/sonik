use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use bincode::{deserialize_from, serialize_into};
use hashbrown::HashMap;
use ignore::{DirEntry, Walk};
use simsearch::SimSearch;

use crate::application::config::Config;
use crate::storage::record::{Album, Artist, Record, Track};
use crate::storage::terms::{SearchQuery, Term};

pub struct EngineGroup {
    pub artists: Engine,
    pub albums: Engine,
    pub tracks: Engine,
}

pub enum Engine {
    Artists(SimSearch<usize>),
    Albums(SimSearch<(usize, usize)>),
    Tracks(SimSearch<(usize, usize, usize)>),
}

impl Engine {
    pub fn search(&self, query_str: &str) -> SearchResult {
        match self {
            Engine::Artists(e) => SearchResult::Artists(e.search(query_str)),
            Engine::Albums(e) => SearchResult::Albums(e.search(query_str)),
            Engine::Tracks(e) => SearchResult::Tracks(e.search(query_str)),
        }
    }
}

type ArtistResult = Vec<usize>;
type AlbumResult = Vec<(usize, usize)>;
type TrackResult = Vec<(usize, usize, usize)>;

pub enum SearchResult {
    Artists(ArtistResult),
    Albums(AlbumResult),
    Tracks(TrackResult),
}

fn is_music(entry: &DirEntry) -> bool {
    let metadata = fs::metadata(entry.path()).unwrap();
    if metadata.is_dir() {
        return false;
    }

    // If the filename isn't a suitable audio format, return false
    if let Some(extension) = entry.path().extension() {
        match extension.to_str() {
            Some("mp3") => return true,
            Some("flac") => return true,
            Some("ogg") => return true,
            _ => return false,
        };
    } else {
        return false;
    }
}

pub fn create_and_load_database(config: &Config) -> Result<Vec<Artist>, ()> {
    // create vector of artists
    let mut artists: Vec<Artist> = Vec::new();

    // Walk through the music directory and add paths for each track
    for result in Walk::new(&config.music_folder) {
        if let Ok(entry) = result {
            if is_music(&entry) {
                let track = Track::new(entry.into_path());
                if let Ok(t) = track {
                    add_to_database_helper(t, &mut artists)
                }
            }
        }
    }

    let mut f = BufWriter::new(
        fs::File::create(&config.database_path).expect("Could not write to database path"),
    );

    // Sort for easy finding in the UI
    artists.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    serialize_into(&mut f, &artists).expect("Could not serialize database to file");

    Ok(artists)
}

pub fn load_database(config: &Config) -> Result<Vec<Artist>, ()> {
    let mut library_reader = BufReader::new(
        fs::File::open(&config.database_path).expect("Could not open database file"),
    );

    let artists = deserialize_from(&mut library_reader).expect("Could not deserialize");

    Ok(artists)
}

fn add_to_database_helper(t: Track, artists: &mut Vec<Artist>) {
    // Copy the string information out of the track and pass it
    // to add_to_database along with the actual track struct

    let artist_name = t.album_artist.clone();
    let album_title = t.album.clone();
    let album_year = t.year;

    add_to_database(&artist_name, &album_title, album_year, t, artists);
}

fn add_to_database(
    artist_name: &str,
    album_title: &str,
    album_year: i32,
    t: Track,
    artists: &mut Vec<Artist>,
) {
    // Strings should be copies of information in track
    // Use them to add/check artists/albums and add track

    // Find an artist that matches the artist name
    let artist_index = artists.iter().position(|a| a.title == artist_name);

    match artist_index {
        // If there is an artist that matches that name...
        Some(idx) => {
            let album_index = artists[idx]
                .albums
                .iter()
                .position(|al| al.title == album_title);
            match album_index {
                Some(al_idx) => if let Ok(()) = artists[idx].albums[al_idx].update_album(t) {},

                None => {
                    // If not, create the album and add the track
                    let mut album =
                        Album::new(album_title.to_string(), artist_name.to_string(), album_year)
                            .unwrap();
                    //debug - println!("Created new album: {}", album_title);
                    album.tracks.push(t);
                    if let Ok(()) = artists[idx].add_album(album) {}
                }
            }
        }

        // If no artist matches that name, then create the artist and album, and add track
        None => {
            let mut artist = Artist::new(artist_name.to_string()).unwrap();
            //debug - println!("Created new artist: {}", &artist.name);

            let mut album =
                Album::new(album_title.to_string(), artist_name.to_string(), album_year).unwrap();
            //debug - println!("Created new album: {}", &album.title);
            album.tracks.push(t);
            if let Ok(()) = artist.add_album(album) {}
            artists.push(artist);
        }
    }
}

pub fn create_search_map<R: Record>(
    records: &[R],
    save_path: &Path,
) -> Result<HashMap<String, usize>, ()> {
    let mut search_map = HashMap::new();

    for (i, record) in (&records).iter().enumerate() {
        let name = record.name();
        search_map.insert(name.to_lowercase(), i);
    }

    let mut map_file =
        BufWriter::new(fs::File::create(save_path).expect("Could not write to map path"));

    serialize_into(&mut map_file, &search_map).expect("Could not serialize map to file");

    Ok(search_map)
}

pub fn load_search_map(file_path: &Path) -> Result<HashMap<String, usize>, ()> {
    let mut map_reader =
        BufReader::new(fs::File::open(&file_path).expect("Could not open map file"));

    let search_map = deserialize_from(&mut map_reader).expect("Could not deserialize");

    Ok(search_map)
}

pub fn create_fuzzy_searcher(records: &[Artist]) -> Result<EngineGroup, ()> {
    let mut artists: SimSearch<usize> = SimSearch::new();
    let mut albums: SimSearch<(usize, usize)> = SimSearch::new();
    let mut tracks: SimSearch<(usize, usize, usize)> = SimSearch::new();

    for (i, record) in (&records).iter().enumerate() {
        let artist_name = &record.title;
        artists.insert(i, &artist_name);
        for (j, album) in (&record.albums).iter().enumerate() {
            let album_name = &album.title;
            albums.insert((i, j), &album_name);
            for (k, track) in (&album.tracks).iter().enumerate() {
                let track_name = &track.title;
                tracks.insert((i, j, k), &track_name);
            }
        }
    }

    Ok(EngineGroup {
        artists: Engine::Artists(artists),
        albums: Engine::Albums(albums),
        tracks: Engine::Tracks(tracks),
    })
}

pub fn search(engine: &EngineGroup, query: SearchQuery) -> SearchResult {
    match query.terms {
        Term::Title(s) => engine.tracks.search(s.as_str()),
        Term::Album(s) => engine.albums.search(s.as_str()),
        Term::Artist(s) => engine.artists.search(s.as_str()),
    }
}
