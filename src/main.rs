use bevy::prelude::*;
use forge_viewer::ForgeViewerPlugin;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb_u8(20, 22, 25)))
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Forge Viewer".into(),
                ..Default::default()
            }),
            ..Default::default()
        })
            .set(AssetPlugin {
                watch_for_changes_override: Some(true),
                ..Default::default()
            })
        )
        .add_plugins(ForgeViewerPlugin)
        .run();
}
