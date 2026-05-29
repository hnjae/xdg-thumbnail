# xdg-thumbnail

Freedesktop thumbnail cache primitives for Rust applications.

The crate provides typed APIs for resolving the personal thumbnail cache, constructing canonical original URI identities, validating existing cache entries, reading validated thumbnail PNG bytes, and atomically installing caller-rendered thumbnails. It does not decode original source formats, choose thumbnailers, or apply cleanup policy.

This crate supports Unix-like XDG desktop environments only. Non-Unix targets fail during crate compilation because thumbnail identity, cache permissions, and local path URI construction depend on Unix path bytes and metadata semantics.

## Example

```rust
use xdg_thumbnail::{
    PersonalCacheRoot, PersonalThumbnailLookup, ReadableOriginalIdentity, ThumbnailSize,
};

fn main() -> xdg_thumbnail::Result<()> {
    let root = PersonalCacheRoot::resolve_from_env()?;
    let original = ReadableOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;

    match root.validated_personal_bytes(&original, ThumbnailSize::Normal)? {
        PersonalThumbnailLookup::Valid(entry) => {
            let _png_bytes = entry.bytes();
        }
        PersonalThumbnailLookup::Missing | PersonalThumbnailLookup::Invalid(_) => {}
        _ => {}
    }

    Ok(())
}
```
