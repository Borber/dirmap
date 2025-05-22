use dirmap::map;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let start_path = if args.len() > 1 {
        &args[1]
    } else {
        eprintln!("请提供起始路径");
        std::process::exit(1);
    };

    let raw = map(start_path)
        .inspect_err(|e| eprintln!("错误：{e}"))
        .expect("映射失败");
    std::fs::write("map", raw).expect("写入文件失败");
}
