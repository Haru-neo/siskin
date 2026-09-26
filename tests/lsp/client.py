import subprocess, json, sys, os, tempfile, urllib.parse
# 윈도우에서도 한글을 그대로 쓰고 읽게 합니다.
sys.stdout.reconfigure(encoding="utf-8")
p = subprocess.Popen([sys.argv[1], "lsp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def send(m):
    b = json.dumps(m).encode()
    p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(b) + b); p.stdin.flush()
def recv():
    n = None
    while True:
        l = p.stdout.readline().decode().strip()
        if not l: break
        if l.lower().startswith("content-length:"): n = int(l.split(":")[1])
    return json.loads(p.stdout.read(n))
d = os.path.join(tempfile.gettempdir(), "lsp").replace("\\", "/")
uri = "file://" + ("" if d.startswith("/") else "/") + urllib.parse.quote(d + "/메인.skn", safe="/:")
text = 'import util\n\nfn Main():\n    let 이름 = "가" + 1\n    print(str(ADD(1, 2)))\n  let bad = 3\n'
os.makedirs(d, exist_ok=True)
open(d + "/util.skn", "w", encoding="utf-8").write(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "util.skn"), encoding="utf-8").read())
open(d + "/메인.skn", "w", encoding="utf-8").write(text)
send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}); print("init", recv()["result"]["serverInfo"])
send({"jsonrpc":"2.0","method":"initialized","params":{}})
send({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"siskin","version":1,"text":text}}})
d = recv(); print("diag", json.dumps(d["params"]["diagnostics"], ensure_ascii=False))
text = 'import util\n\nfn Main():\n    let 이름 = "가" + 1\n    print(str(ADD(1, 2)))\n'
send({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":text}]}})
d = recv(); print("diag2", json.dumps(d["params"]["diagnostics"], ensure_ascii=False))
text2 = 'import util\n\nfn main():\n    let x=add( 1,2 )\n    print(str(x))\n'
send({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":text2}]}})
d = recv(); print("diag3", d["params"]["diagnostics"])
send({"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":True}}})
print("fmt", json.dumps(recv()["result"], ensure_ascii=False))
send({"jsonrpc":"2.0","id":3,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":uri}}})
print("sym", recv()["result"])
send({"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":3,"character":11}}})
print("def", recv()["result"])
send({"jsonrpc":"2.0","id":5,"method":"textDocument/hover","params":{"textDocument":{"uri":uri},"position":{"line":3,"character":11}}})
print("hover", json.dumps(recv()["result"], ensure_ascii=False))
send({"jsonrpc":"2.0","id":6,"method":"textDocument/completion","params":{"textDocument":{"uri":uri},"position":{"line":3,"character":5}}}); r=recv()["result"]; print("comp", len(r), r[0], r[1])
send({"jsonrpc":"2.0","id":7,"method":"shutdown"}); print("shut", recv())
send({"jsonrpc":"2.0","method":"exit"}); print("exit code", p.wait())
