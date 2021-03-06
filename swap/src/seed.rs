use crate::fs::ensure_directory_exists;
use ::bitcoin::secp256k1::{self, constants::SECRET_KEY_SIZE, SecretKey};
use anyhow::Result;
use bdk::bitcoin::util::bip32::ExtendedPrivKey;
use pem::{encode, Pem};
use rand::prelude::*;
use std::{
    ffi::OsStr,
    fmt,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const SEED_LENGTH: usize = 32;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Seed([u8; SEED_LENGTH]);

impl Seed {
    pub fn random() -> Result<Self, Error> {
        let mut bytes = [0u8; SECRET_KEY_SIZE];
        rand::thread_rng().fill_bytes(&mut bytes);

        // If it succeeds once, it'll always succeed
        let _ = SecretKey::from_slice(&bytes)?;

        Ok(Seed(bytes))
    }

    pub fn extended_private_key(&self, network: bitcoin::Network) -> Result<ExtendedPrivKey> {
        let private_key = ExtendedPrivKey::new_master(network, &self.bytes())?;
        Ok(private_key)
    }

    pub fn bytes(&self) -> [u8; SEED_LENGTH] {
        self.0
    }

    pub fn from_file_or_generate(data_dir: &Path) -> Result<Self, Error> {
        let file_path_buf = data_dir.join("seed.pem");
        let file_path = Path::new(&file_path_buf);

        if file_path.exists() {
            return Self::from_file(&file_path);
        }

        tracing::info!("No seed file found, creating at: {}", file_path.display());

        let random_seed = Seed::random()?;
        random_seed.write_to(file_path.to_path_buf())?;

        Ok(random_seed)
    }

    fn from_file<D>(seed_file: D) -> Result<Self, Error>
    where
        D: AsRef<OsStr>,
    {
        let file = Path::new(&seed_file);
        let contents = fs::read_to_string(file)?;
        let pem = pem::parse(contents)?;

        tracing::info!("Read in seed from file: {}", file.display());

        Self::from_pem(pem)
    }

    fn from_pem(pem: pem::Pem) -> Result<Self, Error> {
        if pem.contents.len() != SEED_LENGTH {
            Err(Error::IncorrectLength(pem.contents.len()))
        } else {
            let mut array = [0; SEED_LENGTH];
            for (i, b) in pem.contents.iter().enumerate() {
                array[i] = *b;
            }

            Ok(Self::from(array))
        }
    }

    fn write_to(&self, seed_file: PathBuf) -> Result<(), Error> {
        ensure_directory_exists(&seed_file)?;

        let data = self.bytes();
        let pem = Pem {
            tag: String::from("SEED"),
            contents: data.to_vec(),
        };

        let pem_string = encode(&pem);

        let mut file = File::create(seed_file)?;
        file.write_all(pem_string.as_bytes())?;

        Ok(())
    }
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Seed([*****])")
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<[u8; SEED_LENGTH]> for Seed {
    fn from(bytes: [u8; SEED_LENGTH]) -> Self {
        Seed(bytes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Secp256k1: ")]
    Secp256k1(#[from] secp256k1::Error),
    #[error("io: ")]
    Io(#[from] io::Error),
    #[error("PEM parse: ")]
    PemParse(#[from] pem::PemError),
    #[error("expected 32 bytes of base64 encode, got {0} bytes")]
    IncorrectLength(usize),
    #[error("RNG: ")]
    Rand(#[from] rand::Error),
    #[error("no default path")]
    NoDefaultPath,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn generate_random_seed() {
        let _ = Seed::random().unwrap();
    }

    #[test]
    fn seed_byte_string_must_be_32_bytes_long() {
        let _seed = Seed::from(*b"this string is exactly 32 bytes!");
    }

    #[test]
    fn seed_from_pem_works() {
        let payload: &str = "syl9wSYaruvgxg9P5Q1qkZaq5YkM6GvXkxe+VYrL/XM=";

        // 32 bytes base64 encoded.
        let pem_string: &str = "-----BEGIN SEED-----
syl9wSYaruvgxg9P5Q1qkZaq5YkM6GvXkxe+VYrL/XM=
-----END SEED-----
";

        let want = base64::decode(payload).unwrap();
        let pem = pem::parse(pem_string).unwrap();
        let got = Seed::from_pem(pem).unwrap();

        assert_eq!(got.bytes(), *want);
    }

    #[test]
    fn seed_from_pem_fails_for_short_seed() {
        let short = "-----BEGIN SEED-----
VnZUNFZ4dlY=
-----END SEED-----
";
        let pem = pem::parse(short).unwrap();
        match Seed::from_pem(pem) {
            Ok(_) => panic!("should fail for short payload"),
            Err(e) => {
                match e {
                    Error::IncorrectLength(_) => {} // pass
                    _ => panic!("should fail with IncorrectLength error"),
                }
            }
        }
    }

    #[test]
    #[should_panic]
    fn seed_from_pem_fails_for_long_seed() {
        let long = "-----BEGIN SEED-----
mbKANv2qKGmNVg1qtquj6Hx1pFPelpqOfE2JaJJAMEg1FlFhNRNlFlE=
mbKANv2qKGmNVg1qtquj6Hx1pFPelpqOfE2JaJJAMEg1FlFhNRNlFlE=
-----END SEED-----
";
        let pem = pem::parse(long).unwrap();
        match Seed::from_pem(pem) {
            Ok(_) => panic!("should fail for long payload"),
            Err(e) => {
                match e {
                    Error::IncorrectLength(_) => {} // pass
                    _ => panic!("should fail with IncorrectLength error"),
                }
            }
        }
    }

    #[test]
    fn round_trip_through_file_write_read() {
        let tmpfile = temp_dir().join("seed.pem");

        let seed = Seed::random().unwrap();
        seed.write_to(tmpfile.clone())
            .expect("Write seed to temp file");

        let rinsed = Seed::from_file(tmpfile).expect("Read from temp file");
        assert_eq!(seed.0, rinsed.0);
    }
}
