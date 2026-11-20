pub fn usage() -> &'static str {
    "Usage:
  elyra [flags] <file> [args...]
  elyra run [flags] <file> [args...]
  elyra compile <file>
  elyra asm <file>
  elyra repl [flags]
  elyra version

Flags (any position):
  -h, --help                 Show help
  -v, --version              Show version
  -O<level> or -O <level>    Optimization level: 0,1,2,3, none, basic, standard, aggressive
  -o, --output <path>        Output path (compile/asm)
  --stdout                   Print asm to stdout (asm)
  -ae.<k>=<v>                VM option (e.g., -ae.max-heap=64M)
  --ae-<k>=<v>               VM option (e.g., --ae-max-heap=64M)
  --max-instructions <n>     Stop after n bytecode instructions
  --timeout-ms <n>           Stop after n milliseconds

Warning flags:
  -Wall                      Enable all warnings
  -Werror                    Treat warnings as errors
  -W<category>               Enable specific category (inline, unused, deprecated, shadow)
  -Wno-<category>            Disable specific category

Examples:
  elyra main.elyra -O2 '-ae.max-heap=256M'  (quote in PowerShell)
  elyra run -O3 main.elyra arg1 arg2
  elyra repl -ae.max-heap=1G
  elyra asm main.elyra --stdout
  elyra compile main.elyra -o main.avbc -Wall -Werror
  elyra run program.avbc"
}
