use bevy::prelude::*;
mod player;
use crate::player::PlayerPlugin;
fn main() {
    App::new()
        .insert_resource(ClearColor(Color::WHITE))
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "src/assets".into(),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_plugins(PlayerPlugin)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}
