// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use xdg_thumbnail::ThumbnailSize;

fn main() {
    let sizes = ThumbnailSize::all()
        .map(ThumbnailSize::directory_name)
        .join(", ");

    println!("xdg-thumbnail {} ({sizes})", env!("CARGO_PKG_VERSION"));
}
