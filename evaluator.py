"""
./evaluator examples >>> '["seed1", "seed2", ...]'
./evaluator generate <seed> >>> '{"input": "...", "data": ...}'
./evaluator check <seed> <<< '{"data": ..., "output": "..."}' >>> '{"ok": ..., "reason": ...}'
"""


import json
import random
import sys
from dataclasses import dataclass
from typing import Self

_examples = {}


def example(f):
    _examples[f"_{len(_examples)}"] = f
    return f


###


@dataclass
class Input:
    n: int

    @classmethod
    def from_seed(cls, seed) -> Self:
        random.seed(seed)
        return cls(random.randint(10, 1000))

    def serialize(self) -> str:
        return str(self.n)

    def data(self) -> int:
        return self.n * (self.n + 1) // 2


@dataclass
class Output:
    sum: int

    @classmethod
    def deserialize(cls, s: str) -> Self:
        return cls(int(s))

    def check(self, data, log) -> bool:
        if self.sum < data:
            log("too low")
            return False
        if self.sum > data:
            log("too high")
            return False
        return True


example(lambda: Input(5))
example(lambda: Input(10))
example(lambda: Input.from_seed(0))
example(lambda: Input.from_seed(1))
example(lambda: Input.from_seed(2))

###


if sys.argv[1] == "examples":
    print(json.dumps([*_examples]))
elif sys.argv[1] == "generate":
    seed = sys.argv[2]
    if (inp := _examples.get(seed)) is not None:
        inp = inp()
    else:
        inp = Input.from_seed(seed)
    print(json.dumps({"input": inp.serialize(), "data": inp.data()}))
elif sys.argv[1] == "check":
    with open(0) as f:
        obj = json.load(f)
    out = obj["output"]
    data = obj["data"]
    try:
        out = Output.deserialize(out)
    except:
        print(json.dumps({"ok": False, "reason": "Failed to parse output"}))
    else:
        logs = []
        ok = out.check(data, logs.append)
        print(json.dumps({"ok": ok, "reason": "\n".join(logs)}))

"""
import json
import random
import sys
from dataclasses import dataclass
from typing import Any, Self

random.seed(sys.argv[2])


@dataclass
class Input:
    n: int
    data: Any

    def serialize(self) -> str:
        return str(self.n)


@dataclass
class Output:
    sum: int

    @classmethod
    def deserialize(cls, s: str) -> Self:
        return cls(int(s))


if sys.argv[3] == "generate":
    num = random.randint(10, 1000)
    ans = num * (num + 1) // 2
    print(json.dumps({"input": str(num), "data": {"ans": ans}}))
elif sys.argv[3] == "check":
    inp = json.load(open(0))
    try:
        out = int(inp["output"])
    except ValueError:
        print(json.dumps({"ok": False, "reason": "not a valid integer"}))
    else:
        if out == inp["data"]["ans"]:
            print(json.dumps({"ok": True}))
        elif out < inp["data"]["ans"]:
            print(json.dumps({"ok": False, "reason": "too low"}))
        else:
            print(json.dumps({"ok": False, "reason": "too high"}))
"""
