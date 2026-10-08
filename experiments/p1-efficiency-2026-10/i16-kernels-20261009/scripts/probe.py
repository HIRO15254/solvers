"""Compare conversion loops extracted from the base and current source."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[4]
EXP = Path(__file__).resolve().parents[1]
source = (ROOT / 'crates/hu-engine/src/storage.rs').read_text(encoding='utf-8')
base = subprocess.check_output(['git', 'show', 'c09c0af:crates/hu-engine/src/storage.rs'], cwd=ROOT, text=True, encoding='utf-8')
old = base[base.index('fn quantize_block('):base.index('// Shared op bodies for the i16 backend')]
old = old.replace('fn quantize_block(', 'fn old_quantize(')
current = source[source.index('fn quantize_block('):source.index('/// Fuse dequantization/update')]
function, helper = current.split('#[inline]', 1)
flat = function.replace('fn quantize_block(', 'fn flat_quantize(')
chunk = function[:function.index('    for (dst, &src)')]
chunk += '''    let (dst, dst_tail) = out_q.as_chunks_mut::<8>();
    let (src, src_tail) = vals.as_chunks::<8>();
    for (dst, src) in dst.iter_mut().zip(src) {
        for lane in 0..8 {
            dst[lane] = quantize_value(src[lane], inverse);
        }
    }
    for (dst, &src) in dst_tail.iter_mut().zip(src_tail) {
        *dst = quantize_value(src, inverse);
    }
    scale
}
'''
chunk = chunk.replace('fn quantize_block(', 'fn chunk_quantize(')
program = old + chunk + flat + '#[inline]' + helper + '''
fn main() {
    use std::{hint::black_box, time::Instant};
    let vals: Vec<f32> = (0..3978).map(|i| (i % 101) as f32 - 50.0).collect();
    let mut out = vec![0; vals.len()];
    for rep in 1..=3 {
        for version in 0..3 {
            let start = Instant::now();
            for _ in 0..100000 {
                black_box(match black_box(version) {
                    0 => old_quantize(black_box(&vals), black_box(&mut out)),
                    1 => chunk_quantize(black_box(&vals), black_box(50.0), black_box(&mut out)),
                    _ => flat_quantize(black_box(&vals), black_box(50.0), black_box(&mut out)),
                });
            }
            println!("{} {} {}", rep, version, start.elapsed().as_secs_f64());
        }
    }
}
'''
scratch = ROOT / 'runs/i16-kernels/conversion-probe.rs'
scratch.write_text(program, encoding='utf-8')
exe = ROOT / 'target/conversion-probe.exe'
subprocess.run(['rustc', scratch, '-O', '-C', 'target-cpu=native', '-o', exe], check=True)
output = subprocess.check_output([exe], text=True)
(EXP / 'raw/conversion-probe.txt').write_text(output)
print(output)
