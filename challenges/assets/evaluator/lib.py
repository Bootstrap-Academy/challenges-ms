import json
import random
import sys
from dataclasses import dataclass
from typing import Self

_examples = []


def example(f):
    _examples.append(f)
    return f


def main(Input, Output, prepare=None):
    if sys.argv[1] == "examples":
        print(json.dumps([f"_ex_{x}" for x in range(len(_examples))]))
    elif sys.argv[1] == "generate":
        seed = sys.argv[2]
        if seed[:4] == "_ex_":
            inp = _examples[int(seed[4:])]()
        else:
            inp = Input.from_seed(seed)
        print(json.dumps({"input": inp.serialize(), "data": inp.data()}))
    elif sys.argv[1] == "prepare":
        with open(0) as f:
            obj = json.load(f)
        env = obj["environment"]
        code = obj["code"]
        data = obj["data"]
        if prepare is None:
            print(json.dumps({"code": code, "reason": ""}))
            sys.exit()
        logs = []
        try:
            out = prepare(env, code, data, logs.append)
        except:
            print(json.dumps({"code": None, "reason": "\n".join(logs)}))
        else:
            print(json.dumps({"code": out, "reason": "\n".join(logs)}))
    elif sys.argv[1] == "check":
        with open(0) as f:
            obj = json.load(f)
        out = obj["output"]
        data = obj["data"]
        logs = []
        try:
            out = Output.deserialize(out, logs.append)
        except:
            print(json.dumps({"verdict": "INVALID_OUTPUT_FORMAT", "reason": "\n".join(logs)}))
        else:
            ok = out.check(data, logs.append)
            print(json.dumps({"verdict": "OK" if ok else "WRONG_ANSWER", "reason": "\n".join(logs)}))
