# Library API Specification

The `xdg-thumbnail` library provides reusable primitives for applications that need to read, validate, create, or inspect Freedesktop thumbnails.

## Public API Goals

- Make spec-compatible thumbnail path calculation straightforward.
- Allow applications to validate cached thumbnails without duplicating PNG metadata parsing.
- Allow applications to create thumbnails atomically after they render image data.
- Allow management tools to inspect cache entries and make cleanup decisions without embedding CLI behavior into the library.

## Core Capabilities

The library should expose APIs for:

- Resolving the personal thumbnail cache root.
- Computing the cache path for a canonical original URI and requested thumbnail size.
- Parsing thumbnail PNG metadata.
- Building standard metadata for a newly generated thumbnail.
- Checking whether a cached thumbnail is valid for a given original.
- Iterating cache entries from known thumbnail directories.
- Saving a completed PNG to the cache through an atomic temporary file flow.

## Image Generation Boundary

The core library should not initially decode images, render documents, or extract video frames. Instead, it should accept already-rendered thumbnail image data plus metadata and save it according to the standard.

A later optional feature may provide image-specific helpers, but the base crate should remain small enough for both CLI tools and GUI applications.

## Kiriview Integration Target

Kiriview should be able to use the library through a flow equivalent to:

```rust
let uri = ThumbnailUri::from_file_path(path)?;
let cache_path = cache.thumbnail_path(&uri, ThumbnailSize::Normal);

if cache.is_valid(&cache_path, &uri)? {
    return Ok(cache_path);
}

let rendered_png = render_thumbnail(path)?;
cache.save_png_atomic(&uri, ThumbnailSize::Normal, rendered_png, metadata)?;
```

The exact API should be shaped by implementation, but Kiriview should not need to know the hash filename algorithm, cache directory layout, or PNG metadata keys directly.

