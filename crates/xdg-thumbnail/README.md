# xdg-thumbnail

Freedesktop thumbnail cache primitives for Rust applications.

The crate provides typed APIs for resolving the personal thumbnail cache, constructing canonical original URI identities, validating existing cache entries, reading validated thumbnail PNG bytes, and atomically installing caller-rendered thumbnails. It does not decode original source formats, choose thumbnailers, or apply cleanup policy.

This crate supports Unix-like XDG desktop environments only. Non-Unix targets fail during crate compilation because thumbnail identity, cache permissions, and local path URI construction depend on Unix path bytes and metadata semantics.

`xdg-thumbnail` is pre-release software. Configuration, API shape, and internal formats may change before a stable release.

> [!IMPORTANT]
> This project was developed with AI assistance. Please review the code carefully before relying on it in production.

## Install

```sh
cargo add xdg-thumbnail
```

Or add the dependency manually:

```toml
[dependencies]
xdg-thumbnail = "0.1.0"
```

## Scope

Use this crate when an application already owns source acquisition or rendering and needs Freedesktop-compatible cache primitives:

- Resolve `$XDG_CACHE_HOME/thumbnails` with the XDG fallback rules.
- Construct canonical personal-cache and shared-repository original URI identities.
- Compute cache paths without reading them.
- Validate existing cache entries and return either validated paths or exact validated PNG bytes.
- Parse Freedesktop thumbnail PNG metadata.
- Normalize and atomically install caller-rendered personal-cache thumbnail PNGs.
- Write opt-in personal-cache failure entries.
- Inspect cache entries for management tools without applying cleanup policy.

The crate does not decode original source formats, select thumbnailer helpers, execute external renderers, decide retry policy, or delete cache entries except through explicit inspection handles.

## Lookup Example

```rust
use xdg_thumbnail::{
    PersonalCacheRoot, PersonalThumbnailLookup, ReadablePersonalOriginalIdentity, ThumbnailSize,
};

fn main() -> xdg_thumbnail::Result<()> {
    let root = PersonalCacheRoot::resolve_from_env()?;
    let original = ReadablePersonalOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;

    match root.lookup_thumbnail_png_bytes(&original, ThumbnailSize::Normal)? {
        PersonalThumbnailLookup::Valid(entry) => {
            let _png_bytes = entry.png_bytes();
        }
        PersonalThumbnailLookup::Missing | PersonalThumbnailLookup::Invalid(_) => {}
    }

    Ok(())
}
```

## Install Example

```rust
use xdg_thumbnail::{PersonalCacheRoot, ReadablePersonalOriginalIdentity, ThumbnailSize};

fn main() -> xdg_thumbnail::Result<()> {
    let root = PersonalCacheRoot::resolve_from_env()?;
    let original = ReadablePersonalOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;
    let rendered_png = render_thumbnail_png();
    let installed = root.install_thumbnail_returning_png_bytes(
        &original,
        ThumbnailSize::Normal,
        &rendered_png,
    )?;

    let _cache_path = installed.path();
    let _final_png_bytes = installed.png_bytes();
    Ok(())
}

fn render_thumbnail_png() -> Vec<u8> {
    unimplemented!("return PNG bytes produced by the caller's renderer")
}
```

## Blocking Adapter Example

Owned request constructors only assemble values and perform no filesystem I/O. Local original identity construction reads filesystem metadata, so async applications should do it inside a runtime-specific blocking adapter.

```rust
use xdg_thumbnail::{
    PersonalCacheRoot, PersonalThumbnailInstallRequest, ReadablePersonalOriginalIdentity, ThumbnailSize,
};

fn spawn_blocking<F, R>(operation: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    operation()
}

fn main() -> xdg_thumbnail::Result<()> {
    let root = PersonalCacheRoot::resolve_from_env()?;
    let rendered_png = render_thumbnail_png();

    let installed = spawn_blocking(move || {
        let original = ReadablePersonalOriginalIdentity::from_local_path("/home/alice/Pictures/photo.png")?;
        let request = PersonalThumbnailInstallRequest::new(
            root,
            original,
            ThumbnailSize::Normal,
            rendered_png,
        );
        request.install_png_bytes()
    })?;

    let _cache_path = installed.path();
    Ok(())
}

fn render_thumbnail_png() -> Vec<u8> {
    unimplemented!("return PNG bytes produced by the caller's renderer")
}
```

## Links

- API documentation: <https://docs.rs/xdg-thumbnail>
- Repository: <https://github.com/hnjae/xdg-thumbnail>
- Freedesktop Thumbnail Managing Standard: <https://specifications.freedesktop.org/thumbnail/latest/>
- License: MPL-2.0
