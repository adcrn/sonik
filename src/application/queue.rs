use rand::thread_rng;
use rand::seq::SliceRandom;

use crate::database::track::Track;

pub struct SonikQueue {
    pub tracks: Vec<Track>,
    pub total_time: u32,
}

impl SonikQueue {
    pub fn new() -> SonikQueue {
        SonikQueue {
            tracks: Vec::<Track>::new(),
            total_time: 0,
        }
    }

    pub fn add(&mut self, track: Track) {
        self.total_time += &track.duration;
        self.tracks.push(track);
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.total_time = 0;
    }

    pub fn shuffle(&mut self) {
        self.tracks.shuffle(&mut thread_rng());
    }
}
