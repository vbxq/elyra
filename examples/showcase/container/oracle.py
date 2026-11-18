import bisect
import collections
import sys


class Task:
    def __init__(self, prio, name):
        self.prio = prio
        self.name = name

    def key(self):
        return (-self.prio, self.name)

    def __str__(self):
        return f"{self.name}(p{self.prio})"


class SortedVec:
    def __init__(self, key):
        self.key = key
        self.items = []
        self.keys = []

    def add(self, x):
        k = self.key(x)
        at = bisect.bisect_right(self.keys, k)
        self.keys.insert(at, k)
        self.items.insert(at, x)

    def take(self):
        if not self.items:
            return None
        self.keys.pop(0)
        return self.items.pop(0)

    def peek(self):
        return self.items[0] if self.items else None

    def __len__(self):
        return len(self.items)


class Ring:
    def __init__(self, cap):
        self.slots = collections.deque()
        self.cap = cap
        self.dropped = 0

    def add(self, x):
        if len(self.slots) == self.cap:
            self.slots.popleft()
            self.dropped += 1
        self.slots.append(x)

    def take(self):
        return self.slots.popleft() if self.slots else None

    def peek(self):
        return self.slots[0] if self.slots else None

    def __len__(self):
        return len(self.slots)


def drain(c):
    out = []
    while len(c):
        out.append(c.take())
    return out


def render(xs):
    return "[" + ", ".join(str(x) for x in xs) + "]"


def main():
    out = []
    nums = SortedVec(lambda n: n)
    for v in [42, 7, 19, -3, 7, 100, 0, 56]:
        nums.add(v)
    out.append(f"sorted ints: len {len(nums)}, smallest {nums.peek()}")
    out.append(f"drained ints: {render(drain(nums))}")
    out.append(f"after drain: empty = {'true' if len(nums) == 0 else 'false'}")

    queue = SortedVec(Task.key)
    for prio, name in [
        (2, "write report"),
        (5, "fix outage"),
        (1, "water plants"),
        (5, "call vendor"),
        (5, "update status page"),
        (3, "review patch"),
    ]:
        queue.add(Task(prio, name))
    queue.add(Task(4, "deploy"))
    out.append(f"tasks queued: {len(queue)}, next up {queue.peek()}")
    first = queue.take()
    out.append(f"handled {first.name}, {len(queue)} left")
    out.append(f"remaining by priority: {render(drain(queue))}")

    recent = Ring(3)
    for v in [1, 2, 3, 4, 5]:
        recent.add(v)
    out.append(f"ring keeps {len(recent)} of 5, dropped {recent.dropped}, oldest {recent.peek()}")
    out.append(f"ring drained: {render(drain(recent))}")

    log = Ring(2)
    for prio, name in [(1, "boot"), (9, "panic"), (2, "reboot")]:
        log.add(Task(prio, name))
    drained = render(drain(log))
    out.append(f"log drained: {drained}, empty = {'true' if len(log) == 0 else 'false'}")
    sys.stdout.write("\n".join(out) + "\n")


main()
