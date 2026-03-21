use std::{collections::HashSet, fmt};

use serde::Deserialize;

pub struct Adj {
    pub n: u32,
    pub data: Vec<bool>,
}

impl Adj {
    pub fn get(&self, i: u32, j: u32) -> bool {
        let idx = (i * self.n + j) as usize;
        return match self.data.get(idx) {
            Some(elem) => *elem,
            None => false,
        };
    }

    pub fn is_connected(&self) -> bool {
        let mut seen = HashSet::<u32>::new();
        let mut queue = Vec::<u32>::with_capacity(self.n as usize);
        queue.push(0);
        seen.insert(0);

        while let Some(i) = queue.pop() {
            for j in 0..self.n {
                if i != j && !seen.contains(&j) && (self.get(i, j) || self.get(j, i)) {
                    seen.insert(j);
                    queue.push(j);
                }
            }
        }

        return seen.len() == self.n as usize;
    }
}

#[derive(Deserialize)]
pub struct RawAdj {
    n: u32,
    matrix: Vec<Vec<u8>>,
}

impl RawAdj {
    pub fn to_valid(&self) -> Result<Adj, AdjError> {
        let n_size = self.n as usize;

        if self.matrix.len() != n_size {
            return Err(AdjError::BadDimensions);
        }

        let cell_count: usize = n_size * n_size;
        let mut flat_vec = vec![false; cell_count];

        for (i, row) in self.matrix.iter().enumerate() {
            if row.len() != n_size {
                return Err(AdjError::BadDimensions);
            }
            for (j, cell) in row.iter().enumerate() {
                let idx = i * n_size + j;
                flat_vec[idx] = *cell != 0;
            }
        }

        let adj = Adj {
            n: self.n,
            data: flat_vec,
        };

        if !adj.is_connected() {
            return Err(AdjError::NotConnected);
        }

        return Ok(adj);
    }
}

#[derive(Debug)]
pub enum AdjError {
    BadDimensions,
    NotConnected,
}

impl fmt::Display for AdjError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AdjError::NotConnected => write!(f, "Adj matrix is not connected"),
            AdjError::BadDimensions => write!(f, "Adj dimensions are invalid"),
        }
    }
}

impl std::error::Error for AdjError {}
