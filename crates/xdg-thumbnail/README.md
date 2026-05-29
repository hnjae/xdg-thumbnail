# xdg-thumbnail

Freedesktop thumbnail cache primitives for Rust applications.

The crate provides typed APIs for resolving the personal thumbnail cache, constructing canonical original URI identities, validating existing cache entries, reading validated thumbnail PNG bytes, and atomically installing caller-rendered thumbnails. It does not decode original source formats, choose thumbnailers, or apply cleanup policy.

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
