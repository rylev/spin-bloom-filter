use anyhow::Result;
use bitvec::prelude::*;
use hash32::Hasher;
use spin_sdk::{
    http::{Request, Response},
    http_component,
    key_value::{self, Store},
};

use core::hash::Hash;
use std::{thread::sleep, time::Duration};

/// A simple Spin HTTP component.
#[http_component]
fn email(req: Request) -> Result<Response> {
    match *req.method() {
        http::Method::GET => available(req),
        http::Method::POST => add(req),
        _ => Ok(http::Response::builder().status(405).body(None).unwrap()),
    }
}

#[derive(serde::Deserialize)]
struct Query {
    email: String,
}

/// Check whether the supplied email is taken or not.
///
/// This is a best effort and might return a 200 when the email is indeed already taken.
///
/// Returns 409 if the email is not available, otherwise 200
fn available(req: Request) -> Result<Response> {
    let query = req.uri().query();
    let Some(query) = query else { anyhow::bail!("no query argument") };
    let query: Query = serde_qs::from_str(query)?;

    let store = key_value::Store::open_default()?;
    let mut filter = get_state(&store)?;

    let response = http::Response::builder();
    let status = match filter.exists(&query.email) {
        Exists::Maybe if expensive_user_lookup(&query.email)? => 409,
        Exists::No | Exists::Maybe => 200,
    };

    Ok(response.status(status).body(None).unwrap())
}

/// Simulate expensive user lookup in the database
///
/// This should be replaced with an actual user lookup
fn expensive_user_lookup(_email: &str) -> Result<bool> {
    sleep(Duration::from_millis(10));
    Ok(true)
}

#[derive(serde::Deserialize)]
struct Body {
    email: String,
}

fn add(req: Request) -> Result<Response> {
    let Some(body) = req.body().as_ref() else { anyhow::bail!("No body")};
    let body: Body = serde_json::from_slice(body)?;
    let store = key_value::Store::open_default()?;
    add_user_to_database(&body.email)?;

    // Since we do not have compare and swap in kv store,
    // it is possible that we are corrupting the state
    let mut state = get_state(&store)?;
    state.insert(&body.email);
    write_state(&store, &state)?;
    Ok(http::Response::builder().status(200).body(None).unwrap())
}

fn add_user_to_database(_email: &str) -> Result<()> {
    // This is where the user would be added to the database
    Ok(())
}

fn get_state(store: &key_value::Store) -> Result<BloomFilter> {
    Ok(match store.get("__state") {
        Ok(e) => BloomFilter::from_vec(e)?,
        Err(key_value::Error::NoSuchKey) => BloomFilter::new(),
        Err(e) => return Err(e.into()),
    })
}

fn write_state(store: &Store, state: &BloomFilter) -> Result<()> {
    let mut v = vec![];
    for chunk in state.array.as_raw_slice() {
        v.extend(chunk.to_be_bytes());
    }
    Ok(store.set("__state", v)?)
}

struct BloomFilter {
    array: bitvec::array::BitArray<[u32; 4]>,
    num: usize,
}

const NUM_BITS: usize = 128;

impl BloomFilter {
    fn new() -> Self {
        Self {
            array: bitvec::bitarr!(u32, LocalBits; 0; NUM_BITS),
            num: 0,
        }
    }

    fn from_vec(e: Vec<u8>) -> Result<Self> {
        if e.len() != 16 {
            anyhow::bail!("corrupted state");
        }
        let mut array = [0u32; 4];
        for (dest, source) in e.chunks_exact(4).zip((&mut array[..]).iter_mut()) {
            *source = (dest[0] as u32) << 24
                | (dest[1] as u32) << 16
                | (dest[2] as u32) << 8
                | dest[3] as u32;
        }
        Ok(Self {
            array: array.try_into().unwrap(),
            num: 0,
        })
    }

    /// Insert element into filter
    fn insert<E>(&mut self, element: E)
    where
        E: Hash,
    {
        self.num += 1;
        let hash1 = murmur3(&element) % NUM_BITS;
        self.array.set(hash1, true);
        let hash2 = fnv(&element) % NUM_BITS;
        self.array.set(hash2, true);
    }

    /// Check whether element does not exist in the filter
    ///
    /// `true` means the element is definitely not in the set
    /// `false` means the element *might* be in the set
    ///
    /// Use `false_positive_prob` to check the liklihood that a `false`
    /// is returned even when the item isn't in the set.
    fn exists<E>(&mut self, element: E) -> Exists
    where
        E: Hash,
    {
        let hash1 = murmur3(&element) % NUM_BITS;
        let hash2 = fnv(&element) % NUM_BITS;
        (!self.array[hash1] || !self.array[hash2])
            .then(|| Exists::No)
            .unwrap_or(Exists::Maybe)
    }

    #[cfg(test)]
    /// The percent likelihood of a false positive
    fn false_positive_percent(&self) -> f32 {
        100.0 * (1.0 - (1.0 - 1.0 / NUM_BITS as f32).powf(2.0 * self.num as f32)).powf(2.0)
    }
}

/// What the filter says about the existence of a key
#[derive(PartialEq, Eq, Debug)]
enum Exists {
    /// The filter can only answer no with 100% certainty
    No,
    /// Otherwise the filter can't be sure
    Maybe,
}

fn murmur3<E>(element: &E) -> usize
where
    E: Hash,
{
    let mut hasher = hash32::Murmur3Hasher::default();
    element.hash(&mut hasher);
    hasher.finish32() as usize
}

fn fnv<E>(element: &E) -> usize
where
    E: Hash,
{
    let mut hasher = hash32::FnvHasher::default();
    element.hash(&mut hasher);
    hasher.finish32() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_check() {
        let mut filter = BloomFilter::new();
        filter.insert("hello");
        assert_eq!(filter.exists("hello"), Exists::Maybe);
        assert_eq!(filter.exists("hallo"), Exists::No);
        assert_eq!(filter.false_positive_percent(), 0.0242237);
    }
}
