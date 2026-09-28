"""開発コマンドが起動したプロセス群の生存期間を管理する。"""

import asyncio
import os
from pathlib import Path
import signal
from contextlib import asynccontextmanager

# 終了を無期限に待たず、Vite・API・ビルドの子孫も回収する。
STOP_TIMEOUT_SECONDS = 5


class Processes:
    def __init__(self):
        self.children = []

    async def start(self, command, *, cwd, env=None, capture=False):
        process = await asyncio.create_subprocess_exec(
            *map(str, command),
            cwd=cwd,
            env=env,
            stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE if capture else None,
            start_new_session=True,
        )
        self.children.append(process)
        return process

    async def run(self, command, *, cwd):
        process = await self.start(command, cwd=cwd)
        if await process.wait() != 0:
            raise RuntimeError(f"コマンドが失敗しました: {' '.join(map(str, command))}")
        self.children.remove(process)

    async def stop(self):
        for process in self.children:
            signal_session(process, signal.SIGTERM)
        try:
            async with asyncio.timeout(STOP_TIMEOUT_SECONDS):
                await asyncio.gather(*(process.wait() for process in self.children))
        except TimeoutError:
            pass
        finally:
            # 親が先に終了しても、そのセッションの子プロセスを残さない。
            for process in self.children:
                signal_session(process, signal.SIGKILL)
            await asyncio.gather(*(process.wait() for process in self.children))


def signal_session(process, number):
    groups = {process.pid}
    # PCAP再生は独自のプロセスグループを作るが、起動元のセッションには属する。
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            pid = int(entry.name)
            if os.getsid(pid) == process.pid:
                groups.add(os.getpgid(pid))
        except ProcessLookupError:
            pass
    for group in groups:
        try:
            os.killpg(group, number)
        except ProcessLookupError:
            pass


@asynccontextmanager
async def managed_processes():
    processes = Processes()
    try:
        yield processes
    finally:
        # Ctrl+Cを連打しても、起動済みの子プロセスは片付ける。
        loop = asyncio.get_running_loop()
        for number in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(number, lambda: None)
        await processes.stop()


async def read_ready(process, *, prefix, timeout):
    async with asyncio.timeout(timeout):
        while line := await process.stdout.readline():
            message = line.decode().strip()
            if message.startswith(prefix):
                return message.removeprefix(prefix)
            print(message, flush=True)
    raise RuntimeError("開発サーバがURLを返す前に終了しました")


async def forward_output(process):
    while line := await process.stdout.readline():
        print(line.decode().rstrip(), flush=True)


async def supervise(services):
    readers = [asyncio.create_task(forward_output(process)) for process in services.values()]
    exits = {asyncio.create_task(process.wait()): name for name, process in services.items()}
    try:
        completed, _ = await asyncio.wait(exits, return_when=asyncio.FIRST_COMPLETED)
        finished = next(iter(completed))
        raise RuntimeError(f"{exits[finished]}が終了しました（終了コード{finished.result()}）")
    finally:
        for task in [*readers, *exits]:
            task.cancel()
        await asyncio.gather(*readers, *exits, return_exceptions=True)
