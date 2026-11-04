fn main() {
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut err = std::io::stderr().lock();
    let mut input = std::io::stdin().lock();
    let env = |key: &str| std::env::var(key).ok();
    let code = rsort::run(
        std::env::args_os().collect(),
        rsort::Io {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            term: rsort::ctx::Terminal::detect(),
            env: &env,
        },
    );
    std::process::exit(rsort::flush_stdout(&mut out, &mut err, code));
}
