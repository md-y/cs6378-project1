use std::fmt;

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

        // TODO: Do adj connected check

        return Ok(Adj {
            n: self.n,
            data: flat_vec,
        });
    }
}

#[derive(Debug)]
pub enum AdjError {
    BadDimensions,
}

impl fmt::Display for AdjError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AdjError::BadDimensions => write!(f, "Adj dimensions are invalid"),
        }
    }
}

impl std::error::Error for AdjError {}
