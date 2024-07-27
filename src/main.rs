

use pal::game::Game;

fn main() {
    let mut pal = Game::init().unwrap();
    pal.run().unwrap();
}
