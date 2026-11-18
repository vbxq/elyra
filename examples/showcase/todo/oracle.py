import pathlib
import re
import sys


class Fail(Exception):
    pass


def number(what, word):
    if not re.fullmatch(r"[+-]?\d+", word):
        raise Fail(f"{what} must be a number, got '{word}'")
    return int(word)


class Project:
    def __init__(self):
        self.tasks = []
        self.history = []
        self.next_id = 1
        self.out = []

    def index_of(self, tid):
        for i, t in enumerate(self.tasks):
            if t["id"] == tid:
                return i
        raise Fail(f"no task #{tid}")

    def run(self, line):
        words = line.split(" ")
        name = words[0]
        if name == "add":
            if len(words) < 3:
                raise Fail("'add' needs a priority and a text")
            prio = number("priority", words[1])
            text = " ".join(words[2:])
            tid = self.next_id
            self.next_id += 1
            self.tasks.append({"id": tid, "prio": prio, "text": text, "done": False})
            self.history.append(("added", tid))
            self.out.append(f"added #{tid} (prio {prio}): {text}")
        elif name in ("done", "rm"):
            if len(words) < 2:
                raise Fail(f"'{name}' needs a task id")
            tid = number("task id", words[1])
            i = self.index_of(tid)
            if name == "done":
                if self.tasks[i]["done"]:
                    raise Fail(f"task #{tid} is already done")
                self.tasks[i]["done"] = True
                self.history.append(("completed", tid))
                self.out.append(f"completed #{tid}: {self.tasks[i]['text']}")
            else:
                task = self.tasks.pop(i)
                self.out.append(f"removed #{tid}: {task['text']}")
                self.history.append(("removed", i, task))
        elif name == "list":
            self.list()
        elif name == "stats":
            total = len(self.tasks)
            done = sum(1 for t in self.tasks if t["done"])
            load = sum(t["prio"] for t in self.tasks if not t["done"])
            percent = 0 if total == 0 else done * 100 // total
            self.out.append(
                f"stats: {total} tasks, {done} done, {total - done} open, "
                f"{percent}% complete, open priority load {load}"
            )
        elif name == "undo":
            if not self.history:
                raise Fail("nothing to undo")
            action = self.history.pop()
            if action[0] == "added":
                task = self.tasks.pop(self.index_of(action[1]))
                self.out.append(f"undo: dropped #{action[1]} ({task['text']})")
            elif action[0] == "completed":
                self.tasks[self.index_of(action[1])]["done"] = False
                self.out.append(f"undo: reopened #{action[1]}")
            else:
                self.tasks.insert(action[1], action[2])
                self.out.append(f"undo: restored #{action[2]['id']}")
        else:
            raise Fail(f"unknown command '{name}'")

    def list(self):
        if not self.tasks:
            self.out.append("(no tasks)")
            return
        rows = sorted(self.tasks, key=lambda t: (t["done"], -t["prio"], t["id"]))
        width = max([4] + [len(t["text"]) for t in rows])
        self.out.append(f"{'ID':>3}  {'PRIO':>4}  {'STATUS':<6}  TASK")
        self.out.append(f"{'-' * 3}  {'-' * 4}  {'-' * 6}  {'-' * width}")
        for t in rows:
            status = "done" if t["done"] else "open"
            self.out.append(f"{t['id']:>3}  {t['prio']:>4}  {status:<6}  {t['text']}")


def main():
    project = Project()
    errors = 0
    for n, raw in enumerate(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        try:
            project.run(line)
        except Fail as e:
            errors += 1
            project.out.append(f"line {n}: {e}")
    project.out.append(f"finished with {errors} errors")
    sys.stdout.write("\n".join(project.out) + "\n")


main()
