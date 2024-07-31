use pal::game::Game;

fn main() {
    let mut pal = Game::new().unwrap();
    pal.run().unwrap();
}
