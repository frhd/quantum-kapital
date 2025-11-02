use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Cache service for storing API responses to disk
pub struct CacheService {
    cache_dir: PathBuf,
    ttl_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedData<T> {
    timestamp: u64,
    data: T,
}

impl CacheService {
    /// Creates a new CacheService with default TTL of 24 hours
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let cache_dir = cache_dir.as_ref().to_path_buf();

        // Create cache directory if it doesn't exist
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
            info!("Created cache directory: {:?}", cache_dir);
        }

        Ok(Self {
            cache_dir,
            ttl_seconds: 24 * 60 * 60, // 24 hours
        })
    }

    /// Creates a new CacheService with custom TTL
    pub fn with_ttl(
        cache_dir: impl AsRef<Path>,
        ttl: Duration,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut service = Self::new(cache_dir)?;
        service.ttl_seconds = ttl.as_secs();
        Ok(service)
    }

    /// Checks if a cached file exists and is still valid
    pub fn is_valid(&self, key: &str) -> bool {
        let cache_path = self.get_cache_path(key);

        if !cache_path.exists() {
            debug!("Cache miss: {} (file does not exist)", key);
            return false;
        }

        // Check if cache is expired
        match self.get_cache_age(&cache_path) {
            Ok(age) => {
                if age > self.ttl_seconds {
                    debug!("Cache expired: {} (age: {}s, ttl: {}s)", key, age, self.ttl_seconds);
                    false
                } else {
                    debug!("Cache hit: {} (age: {}s)", key, age);
                    true
                }
            }
            Err(e) => {
                warn!("Failed to get cache age for {}: {}", key, e);
                false
            }
        }
    }

    /// Reads data from cache
    pub fn read<T>(&self, key: &str) -> Result<T, Box<dyn Error + Send + Sync>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let cache_path = self.get_cache_path(key);

        if !self.is_valid(key) {
            return Err("Cache invalid or expired".into());
        }

        let content = fs::read_to_string(&cache_path)?;
        let cached: CachedData<T> = serde_json::from_str(&content)?;

        info!("Read from cache: {}", key);
        Ok(cached.data)
    }

    /// Writes data to cache
    pub fn write<T>(&self, key: &str, data: &T) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        T: Serialize,
    {
        let cache_path = self.get_cache_path(key);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        let cached = CachedData {
            timestamp,
            data,
        };

        let content = serde_json::to_string_pretty(&cached)?;
        fs::write(&cache_path, content)?;

        info!("Wrote to cache: {}", key);
        Ok(())
    }

    /// Clears a specific cache entry
    pub fn clear(&self, key: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        let cache_path = self.get_cache_path(key);

        if cache_path.exists() {
            fs::remove_file(&cache_path)?;
            info!("Cleared cache: {}", key);
        }

        Ok(())
    }

    /// Clears all expired cache entries
    pub fn clear_expired(&self) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let mut cleared = 0;

        if !self.cache_dir.exists() {
            return Ok(0);
        }

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                match self.get_cache_age(&path) {
                    Ok(age) if age > self.ttl_seconds => {
                        fs::remove_file(&path)?;
                        cleared += 1;
                    }
                    _ => {}
                }
            }
        }

        if cleared > 0 {
            info!("Cleared {} expired cache entries", cleared);
        }

        Ok(cleared)
    }

    /// Clears all cache entries
    pub fn clear_all(&self) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let mut cleared = 0;

        if !self.cache_dir.exists() {
            return Ok(0);
        }

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                fs::remove_file(&path)?;
                cleared += 1;
            }
        }

        info!("Cleared all {} cache entries", cleared);
        Ok(cleared)
    }

    fn get_cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", key))
    }

    fn get_cache_age(&self, path: &Path) -> Result<u64, Box<dyn Error + Send + Sync>> {
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        let age = SystemTime::now()
            .duration_since(modified)?
            .as_secs();
        Ok(age)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::thread;
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        value: String,
    }

    #[test]
    fn test_cache_write_and_read() {
        let temp_dir = TempDir::new().unwrap();
        let cache = CacheService::new(temp_dir.path()).unwrap();

        let data = TestData {
            value: "test".to_string(),
        };

        // Write to cache
        cache.write("test_key", &data).unwrap();

        // Read from cache
        let cached: TestData = cache.read("test_key").unwrap();
        assert_eq!(cached, data);
    }

    #[test]
    fn test_cache_expiration() {
        let temp_dir = TempDir::new().unwrap();
        let cache = CacheService::with_ttl(temp_dir.path(), Duration::from_secs(1)).unwrap();

        let data = TestData {
            value: "test".to_string(),
        };

        // Write to cache
        cache.write("test_key", &data).unwrap();
        assert!(cache.is_valid("test_key"));

        // Wait for expiration
        thread::sleep(Duration::from_secs(2));
        assert!(!cache.is_valid("test_key"));
    }

    #[test]
    fn test_cache_clear() {
        let temp_dir = TempDir::new().unwrap();
        let cache = CacheService::new(temp_dir.path()).unwrap();

        let data = TestData {
            value: "test".to_string(),
        };

        // Write to cache
        cache.write("test_key", &data).unwrap();
        assert!(cache.is_valid("test_key"));

        // Clear cache
        cache.clear("test_key").unwrap();
        assert!(!cache.is_valid("test_key"));
    }
}
