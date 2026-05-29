// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use xdg_thumbnail::{PersonalOriginalUri, SharedRelativeOriginalUri};

#[test]
fn local_path_vectors_match_freedesktop_compatibility_hashes() {
    let cases: &[(&[u8], &str, &str)] = &[
        (
            b"/home/alice/photo.png",
            "file:///home/alice/photo.png",
            "82346fd12242a0f50d9cf25786189951",
        ),
        (
            b"/home/alice/My Photo.png",
            "file:///home/alice/My%20Photo.png",
            "a760eeee894f58795a5fb0ce8e4235f5",
        ),
        (
            b"/home/alice/100%.png",
            "file:///home/alice/100%25.png",
            "c2084e2ae9571339fc37db20ca459ba0",
        ),
        (
            b"/home/alice/~literal.png",
            "file:///home/alice/~literal.png",
            "32434a84374b6e67bb9b949250390257",
        ),
        (
            b"/tmp/\xFF.png",
            "file:///tmp/%FF.png",
            "432dc9e7c3ec5a69b2caad256c9ba799",
        ),
        (
            b"/home/alice/has#hash?query.png",
            "file:///home/alice/has%23hash%3Fquery.png",
            "4da0ebac210a741f1f016c22eb4c94ec",
        ),
        (
            b"/home/alice/a+b=c@d.png",
            "file:///home/alice/a+b=c@d.png",
            "5a57723b293ff32a8946faaf9de5f46a",
        ),
        (
            b"/home/alice/%ff.png",
            "file:///home/alice/%25ff.png",
            "f0f601f81374d3eb5daae240f77148a3",
        ),
    ];

    for (path, expected_uri, expected_stem) in cases {
        let uri = PersonalOriginalUri::from_absolute_path_bytes(path).unwrap();
        let uri_from_path =
            PersonalOriginalUri::from_absolute_path(Path::new(OsStr::from_bytes(path))).unwrap();
        assert_eq!(uri.as_str(), *expected_uri);
        assert_eq!(uri_from_path, uri);
        assert_eq!(uri.thumbnail_filename(), format!("{expected_stem}.png"));
    }
}

#[test]
fn local_path_constructor_rejects_relative_paths() {
    assert!(PersonalOriginalUri::from_absolute_path(Path::new("relative.png")).is_err());
}

#[test]
fn textual_local_file_uri_normalizes_localhost_only() {
    let uri =
        PersonalOriginalUri::from_local_file_uri("file://localhost/home/alice/photo.png").unwrap();

    assert_eq!(uri.as_str(), "file:///home/alice/photo.png");
    assert_eq!(
        uri.thumbnail_filename(),
        "82346fd12242a0f50d9cf25786189951.png"
    );

    let uppercase =
        PersonalOriginalUri::from_local_file_uri("FILE://LOCALHOST/home/alice/photo.png").unwrap();
    assert_eq!(uppercase.as_str(), "file:///home/alice/photo.png");

    let encoded_space =
        PersonalOriginalUri::from_local_file_uri("file:///home/alice/My%20Photo.png").unwrap();
    assert_eq!(encoded_space.as_str(), "file:///home/alice/My%20Photo.png");
    assert_eq!(
        encoded_space.thumbnail_filename(),
        "a760eeee894f58795a5fb0ce8e4235f5.png"
    );

    let lowercase_escape = PersonalOriginalUri::from_local_file_uri("file:///tmp/%ff.png").unwrap();
    assert_eq!(lowercase_escape.as_str(), "file:///tmp/%FF.png");

    assert!(PersonalOriginalUri::from_local_file_uri("file://server/share/photo.png").is_err());
    assert!(PersonalOriginalUri::from_local_file_uri("file:///home/alice/My Photo.png").is_err());
    assert!(PersonalOriginalUri::from_local_file_uri("file:///home/alice/has#hash.png").is_err());
}

#[test]
fn caller_provided_absolute_uri_is_validated_and_preserved() {
    let uri =
        PersonalOriginalUri::from_caller_selected_absolute_uri("smb://server/share/My%20Photo.png")
            .unwrap();

    assert_eq!(uri.as_str(), "smb://server/share/My%20Photo.png");
    assert_eq!(
        uri.thumbnail_filename(),
        "9225e92d750e899fbcc3b764c3085162.png"
    );

    assert!(
        PersonalOriginalUri::from_caller_selected_absolute_uri("file:///home/alice/photo.png")
            .is_err()
    );
    assert!(
        PersonalOriginalUri::from_caller_selected_absolute_uri(
            "file://localhost/home/alice/photo.png"
        )
        .is_err()
    );
    assert!(PersonalOriginalUri::from_caller_selected_absolute_uri("relative/path.png").is_err());
    assert!(
        PersonalOriginalUri::from_caller_selected_absolute_uri("http://example.test/My Photo.png")
            .is_err()
    );
    assert!(
        PersonalOriginalUri::from_caller_selected_absolute_uri(
            "http://example.test/snowman-\u{2603}.png"
        )
        .is_err()
    );
    assert!(
        PersonalOriginalUri::from_caller_selected_absolute_uri("http://example.test/a\nb.png")
            .is_err()
    );
}

#[test]
fn shared_child_vectors_match_compatibility_hashes() {
    let cases: &[(&[u8], &str, &str)] = &[
        (
            b"picture.png",
            "./picture.png",
            "7fd0e41c1612f860427a76c4100745a3",
        ),
        (
            b"My Photo.png",
            "./My%20Photo.png",
            "2d307968e33baf350051fbae83b1ef47",
        ),
        (
            b"100%.png",
            "./100%25.png",
            "47d342b8e9d11c426b2a8fc828a38c81",
        ),
        (
            br"name\part.png",
            "./name%5Cpart.png",
            "d192df08f05de51d721ae04466e0d015",
        ),
        (
            b"dir%2Fpicture.png",
            "./dir%252Fpicture.png",
            "32127d4f320bca2eed708ef2a426b3cf",
        ),
    ];

    for (name, expected_uri, expected_stem) in cases {
        let uri = SharedRelativeOriginalUri::from_raw_child_name(name).unwrap();
        assert_eq!(uri.as_str(), *expected_uri);
        assert_eq!(uri.thumbnail_filename(), format!("{expected_stem}.png"));
    }
}

#[test]
fn shared_text_parser_rejects_encoded_slash_and_parent_segments() {
    assert!(SharedRelativeOriginalUri::parse("./picture.png").is_ok());
    assert_eq!(
        SharedRelativeOriginalUri::parse("./name%5cpart.png")
            .unwrap()
            .as_str(),
        "./name%5Cpart.png"
    );
    assert_eq!(
        SharedRelativeOriginalUri::parse("./%70icture.png")
            .unwrap()
            .as_str(),
        "./picture.png"
    );
    assert!(SharedRelativeOriginalUri::from_raw_child_name(b"dir/picture.png").is_err());
    assert!(SharedRelativeOriginalUri::from_raw_child_name(b".").is_err());
    assert!(SharedRelativeOriginalUri::from_raw_child_name(b"..").is_err());
    assert!(SharedRelativeOriginalUri::parse("./dir%2Fpicture.png").is_err());
    assert!(SharedRelativeOriginalUri::parse("./My Photo.png").is_err());
    assert!(SharedRelativeOriginalUri::parse("./name\\part.png").is_err());
    assert!(SharedRelativeOriginalUri::parse("./").is_err());
    assert!(SharedRelativeOriginalUri::parse("picture.png").is_err());
}
