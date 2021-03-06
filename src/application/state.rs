use std::fs::File;
use std::io::BufReader;

use crossbeam_channel::{Receiver, Sender};
use rodio::{Device, Sink};

use crate::application::queue::SonikQueue;
use crate::storage::database::search as db_search;
use crate::storage::database::{EngineGroup, SearchResult};
use crate::storage::record::{Album, Artist, Media, Stats, Track};
use crate::storage::terms::SearchQuery;

// Tabs only need name and ordering information
pub struct TabsState<'a> {
    pub titles: Vec<&'a str>,
    pub index: usize,
}

// This allows for easy switching between tabs in the UI
impl<'a> TabsState<'a> {
    pub fn new(titles: Vec<&'a str>) -> TabsState {
        TabsState { titles, index: 0 }
    }
    pub fn next(&mut self) {
        self.index = (self.index + 1) % self.titles.len();
    }

    pub fn previous(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        } else {
            self.index = self.titles.len() - 1;
        }
    }
}

// ListState allows for the selection and traversal of different lists
pub struct ListState<I> {
    pub items: Vec<I>,
    pub selected: usize,
}

impl<I> ListState<I>
where
    I: std::clone::Clone,
{
    fn new(items: &[I]) -> ListState<I> {
        ListState {
            items: items.to_vec(),
            selected: 0,
        }
    }

    fn select_previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.items.len() - 1;
        }
    }

    fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }
}

// Associates all of the columns together by artist and album selection
pub struct LibraryCols {
    pub artists: ListState<Artist>,
    pub albums: ListState<Album>,
    pub tracks: ListState<Track>,
    pub current_active: usize,
}

impl LibraryCols {
    pub fn switch_left(&mut self) {
        if self.current_active > 0 {
            self.current_active -= 1;
        }
    }

    pub fn switch_right(&mut self) {
        if self.current_active < 2 {
            self.current_active += 1;
        }
    }

    pub fn on_up(&mut self) {
        // List states need to be refreshed when scrolling through each column
        match self.current_active {
            0 => {
                self.artists.select_previous();
                self.albums = ListState::new(&self.artists.items[self.artists.selected].albums);
                self.tracks = ListState::new(&self.albums.items[self.albums.selected].tracks);
            }
            1 => {
                self.albums.select_previous();
                self.tracks = ListState::new(&self.albums.items[self.albums.selected].tracks);
            }
            2 => self.tracks.select_previous(),
            _ => {}
        };
    }

    pub fn on_down(&mut self) {
        match self.current_active {
            0 => {
                self.artists.select_next();
                self.albums = ListState::new(&self.artists.items[self.artists.selected].albums);
                self.tracks = ListState::new(&self.albums.items[self.albums.selected].tracks);
            }
            1 => {
                self.albums.select_next();
                self.tracks = ListState::new(&self.albums.items[self.albums.selected].tracks);
            }
            2 => self.tracks.select_next(),
            _ => {}
        };
    }
}

// t - track, b - bool, p - play/pause
// Sends information to the UI when audio is not playing,
// and also receives tracks from the queue as well as play/pause events
pub struct Audio {
    pub device: Device,
    pub sink: Sink,
    pub trx: Receiver<Track>,
    pub btx: Sender<bool>,
    pub prx: Receiver<bool>,
}

impl Audio {
    pub fn new(
        device: Device,
        trx: Receiver<Track>,
        btx: Sender<bool>,
        prx: Receiver<bool>,
    ) -> Audio {
        Audio {
            sink: Sink::new(&device),
            device,
            trx,
            btx,
            prx,
        }
    }

    pub fn play(&mut self, track: Track) {
        // The clear function does not work for rodio::Sink, so the
        // sink field is just reassigned and it works just as well
        self.sink = Sink::new(&self.device);
        let file = File::open(&track.file_path).unwrap();
        let source = rodio::Decoder::new(BufReader::new(file)).unwrap();
        self.sink.append(source);
    }

    // Notify the UI that there is no audio playing
    pub fn notify(&mut self) {
        if let Ok(()) = self.btx.send(true) {}
    }

    pub fn pause_play(&mut self) {
        if self.sink.is_paused() {
            self.sink.play();
        } else {
            self.sink.pause();
        }
    }

    pub fn stop(&mut self) {
        self.sink = Sink::new(&self.device);
    }
}

pub struct UI<'a> {
    pub queue: SonikQueue,
    pub should_quit: bool,
    pub tabs: TabsState<'a>,
    pub lib_cols: LibraryCols,
    pub now_playing: Track,
    pub rx: Receiver<bool>,
    pub tx: Sender<Track>,
    pub ptx: Sender<bool>,
    pub search_input: String,
    pub fuzzy_searcher: EngineGroup,
    pub search_results: Vec<Media>,
    pub search_select: usize,
    pub stats: Stats,
}

impl<'a> UI<'a> {
    pub fn new(
        database: &[Artist],
        rx: Receiver<bool>,
        tx: Sender<Track>,
        ptx: Sender<bool>,
        fuzzy_searcher: EngineGroup,
        stats: Stats,
    ) -> UI<'a> {
        // Generate initial list states
        let art_col = ListState::new(database);
        let al_col = ListState::new(&art_col.items[art_col.selected].albums);
        let tr_col = ListState::new(&al_col.items[al_col.selected].tracks);

        // Associate them all together
        let lib_cols = LibraryCols {
            artists: art_col,
            albums: al_col,
            tracks: tr_col,
            current_active: 0,
        };

        UI {
            queue: SonikQueue::new(),
            should_quit: false,
            tabs: TabsState::new(vec!["queue", "library", "search"]),
            lib_cols,
            now_playing: Track::dummy(),
            rx,
            tx,
            ptx,
            search_input: String::new(),
            fuzzy_searcher,
            search_results: Vec::new(),
            search_select: 0,
            stats,
        }
    }

    pub fn on_enter(&mut self) {
        match self.tabs.index {
            1 => {
                if self.lib_cols.current_active == 2 {
                    let track = self.lib_cols.tracks.items[self.lib_cols.tracks.selected].clone();
                    let audio_copy = track.clone();

                    if let Ok(()) = self.tx.send(audio_copy) {}
                    self.now_playing = track;
                } else if self.lib_cols.current_active == 1 {
                    let mut album =
                        self.lib_cols.albums.items[self.lib_cols.albums.selected].clone();
                    let track = album.tracks.remove(0);
                    let audio_copy = track.clone();

                    if let Ok(()) = self.tx.send(audio_copy) {}
                    self.now_playing = track;

                    while let Some(t) = album.tracks.pop() {
                        self.queue.add_to_front(t);
                    }
                }
            }
            2 => {
                if self.search_input == "" && self.search_results.is_empty() {
                } else if self.search_input == "" {
                    match &self.search_results[self.search_select] {
                        Media::Artist(_a) => {}
                        Media::Album(a) => {
                            let mut album = a.clone();
                            let track = album.tracks.remove(0);
                            let audio_copy = track.clone();

                            if let Ok(()) = self.tx.send(audio_copy) {}
                            self.now_playing = track;

                            while let Some(t) = album.tracks.pop() {
                                self.queue.add_to_front(t);
                            }
                        }
                        Media::Track(t) => {
                            let track = t.clone();
                            let audio_copy = track.clone();
                            if let Ok(()) = self.tx.send(audio_copy) {}
                            self.now_playing = track;
                        }
                    }
                } else {
                    self.search();
                }
            }
            _ => {}
        }
    }

    pub fn play_from_queue(&mut self) {
        let track = self.queue.take();
        let audio_copy = track.clone();
        if let Ok(()) = self.tx.send(audio_copy) {}
        self.now_playing = track;
    }

    pub fn pause_play(&mut self) {
        if let Ok(()) = self.ptx.send(true) {}
    }

    pub fn add_to_queue(&mut self) {
        match self.tabs.index {
            1 => {
                if self.lib_cols.current_active == 2 {
                    let track = self.lib_cols.tracks.items[self.lib_cols.tracks.selected].clone();
                    self.queue.add(track);
                } else if self.lib_cols.current_active == 1 {
                    for t in &self.lib_cols.albums.items[self.lib_cols.albums.selected].tracks {
                        self.queue.add(t.clone());
                    }
                } else {
                    for a in &self.lib_cols.artists.items[self.lib_cols.artists.selected].albums {
                        for t in &a.tracks {
                            self.queue.add(t.clone());
                        }
                    }
                }
            }
            2 => match &self.search_results[self.search_select] {
                Media::Artist(a) => {
                    for album in &a.albums {
                        for t in &album.tracks {
                            self.queue.add(t.clone());
                        }
                    }
                }
                Media::Album(a) => {
                    for t in &a.tracks {
                        self.queue.add(t.clone());
                    }
                }
                Media::Track(t) => {
                    let track = t.clone();
                    self.queue.add(track);
                }
            },
            _ => {}
        }
    }

    pub fn add_to_front(&mut self) {
        if let 1 = self.tabs.index {
            if self.lib_cols.current_active == 2 {
                let track = self.lib_cols.tracks.items[self.lib_cols.tracks.selected].clone();
                self.queue.add_to_front(track);
            } else if self.lib_cols.current_active == 1 {
                let mut tracklist = self.lib_cols.albums.items[self.lib_cols.albums.selected]
                    .tracks
                    .clone();
                while let Some(t) = tracklist.pop() {
                    self.queue.add_to_front(t);
                }
            } else {
                for a in &self.lib_cols.artists.items[self.lib_cols.artists.selected].albums {
                    let mut tracklist = a.tracks.clone();
                    while let Some(t) = tracklist.pop() {
                        self.queue.add_to_front(t);
                    }
                }
            }
        }
    }

    pub fn clear_queue(&mut self) {
        if let Ok(()) = self.ptx.send(false) {}
        self.queue.clear();
        self.blank_now_play();
    }

    pub fn blank_now_play(&mut self) {
        match self.now_playing.title.as_ref() {
            "" => {}
            _ => {
                self.now_playing = Track::dummy();
            }
        }
    }

    pub fn search(&mut self) {
        if self.search_input == "" {
            return;
        }

        let query_term = SearchQuery::new(self.search_input.as_str());
        self.search_input = String::new();

        if query_term.is_none() {
            return;
        }

        self.search_results = match db_search(&self.fuzzy_searcher, query_term.unwrap()) {
            SearchResult::Artists(r) => r
                .iter()
                .map(|x| Media::Artist(self.lib_cols.artists.items[*x].clone()))
                .collect(),
            SearchResult::Albums(r) => r
                .iter()
                .map(|x| Media::Album(self.lib_cols.artists.items[x.0].albums[x.1].clone()))
                .collect(),
            SearchResult::Tracks(r) => r
                .iter()
                .map(|x| {
                    Media::Track(self.lib_cols.artists.items[x.0].albums[x.1].tracks[x.2].clone())
                })
                .collect(),
        };
    }

    pub fn on_up_search(&mut self) {
        if self.search_select > 0 {
            self.search_select -= 1;
        } else {
            self.search_select = self.search_results.len() - 1;
        }
    }

    pub fn on_down_search(&mut self) {
        self.search_select = (self.search_select + 1) % self.search_results.len();
    }
}
