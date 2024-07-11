mod mkf;

fn main() {

    
    let mut test_mkf = mkf::open("/home/rocky/Code/Game/PAL95/F.MKF").unwrap();
    println!("testMKF: {:?}", test_mkf);

    for i in 0..test_mkf.chunk_count() {
        println!("Chunk[{}]: {:?}", i, test_mkf.read_chunk(i).unwrap().len());
    }
    /*
    let chunk = test_mkf.read_chunk(3).unwrap();
    println!("Chunk: {:?}", chunk.len());
    */
}
