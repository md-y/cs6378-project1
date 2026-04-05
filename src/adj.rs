use std::{
    cmp::{max, min},
    collections::HashSet,
    fmt,
};

use log::info;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adj {
    pub n: u32,
    pub data: Vec<bool>,
    pub deactivated: HashSet<u32>,
}

impl Adj {
    pub fn get(&self, i: u32, j: u32) -> bool {
        if self.is_deactivated(&i) || self.is_deactivated(&j) {
            return false;
        }
        return self.get_with_deactivated(i, j);
    }

    pub fn get_with_deactivated(&self, i: u32, j: u32) -> bool {
        if i == j {
            return false;
        }
        let idx = (i * self.n + j) as usize;
        return match self.data.get(idx) {
            Some(elem) => *elem,
            None => false,
        };
    }

    pub fn set(&mut self, i: u32, j: u32, val: bool) {
        if i == j {
            return;
        }
        let idx = (i * self.n + j) as usize;
        self.data[idx] = val;
    }

    pub fn get_components(&self) -> Vec<HashSet<u32>> {
        let mut components: Vec<HashSet<u32>> = Vec::new();
        for i in 0..self.n {
            if self.is_activated(&i) {
                components.push(HashSet::from([i]));
            }
        }

        for i in 0..self.n {
            for j in 0..self.n {
                if i == j || (!self.get(i, j) && !self.get(j, i)) {
                    continue;
                }

                let i_c_idx = components.iter().position(|s| s.contains(&i)).unwrap();
                let j_c_idx = components.iter().position(|s| s.contains(&j)).unwrap();
                if i_c_idx == j_c_idx  {
                    continue;
                }

                let lesser = min(i_c_idx, j_c_idx);
                let greater = max(i_c_idx, j_c_idx);
                let removed_set = components.swap_remove(greater);
                components[lesser] = &components[lesser] | &removed_set;
            }
        }

        return components;
    }

    pub fn is_deactivated(&self, node_id: &u32) -> bool {
        return self.deactivated.contains(node_id);
    }

    pub fn is_activated(&self, node_id: &u32) -> bool {
        return !self.is_deactivated(node_id);
    }

    pub fn activate_node(&mut self, node_id: &u32) -> bool {
        return self.deactivated.remove(node_id);
    }

    pub fn deactivate_node(&mut self, node_id: u32) -> bool {
        return self.deactivated.insert(node_id);
    }

    pub fn is_connected(&self) -> bool {
        let mut seen = HashSet::<u32>::new();
        let mut queue = Vec::<u32>::with_capacity(self.n as usize);
        for i in 0..self.n {
            if self.is_activated(&i) {
                queue.push(i);
                seen.insert(i);
                break;
            }
        }

        while let Some(i) = queue.pop() {
            for j in 0..self.n {
                if i != j && !seen.contains(&j) && (self.get(i, j) || self.get(j, i)) {
                    seen.insert(j);
                    queue.push(j);
                }
            }
        }

        let required_count = self.n as usize - self.deactivated.len();
        return seen.len() == required_count;
    }

    pub fn expand(&self, new_n: u32) -> Adj {
        let mut data = vec![false; (new_n * new_n) as usize];
        for i in 0..self.n {
            for j in 0..self.n {
                data[(i * new_n + j) as usize] = self.get(i, j);
            }
        }
        let skipped_nodes = HashSet::from_iter(self.n..new_n);
        return Adj {
            n: new_n,
            data,
            deactivated: &self.deactivated | &skipped_nodes,
        };
    }

    pub fn node_degree(&self, node_id: u32) -> u32 {
        let mut d = 0;
        for i in 0..self.n {
            if self.get(i, node_id) {
                d += 1;
            }
            if self.get(node_id, i) {
                d += 1;
            }
        }
        return d;
    }

    pub fn repair(&mut self) {
        let components = self.get_components();
        if components.len() == 1 {
            return;
        }

        let hub = components.iter().max_by_key(|set| set.len()).unwrap();

        let core = hub.iter().max_by_key(|id| self.node_degree(**id)).unwrap();

        for c in &components {
            if c == hub {
                continue;
            }

            let node = c.iter().max_by_key(|id| self.node_degree(**id)).unwrap();
            self.set(*core, *node, true);
            info!(target: "ADJ", "Repaired Adj by connecting node {} to node {}", core, node);
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct RawAdj {
    pub n: u32,
    pub matrix: Vec<Vec<u8>>,
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
            deactivated: HashSet::new(),
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
