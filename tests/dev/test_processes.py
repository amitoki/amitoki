"""親が先に終了しても、別グループの子を残さないことを確認する。"""

import asyncio
import os
from pathlib import Path
import select
import signal
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from dev_processes import Processes


class ProcessCleanupTests(unittest.IsolatedAsyncioTestCase):
    async def test_a_descendant_in_another_group_is_stopped_after_its_parent_exits(self):
        child_code = "import signal; signal.signal(signal.SIGTERM, signal.SIG_IGN); print('ready', flush=True); signal.pause()"
        parent_code = (
            "import subprocess, sys; "
            f"child = subprocess.Popen([sys.executable, '-c', {child_code!r}], stdout=subprocess.PIPE, process_group=0); "
            "child.stdout.readline(); print(child.pid, flush=True)"
        )
        processes = Processes()
        parent = await processes.start([sys.executable, "-c", parent_code], cwd=ROOT, capture=True)
        child_pid = int(await asyncio.wait_for(parent.stdout.readline(), 10))
        child_exit = os.pidfd_open(child_pid)
        try:
            self.assertEqual(await asyncio.wait_for(parent.wait(), 10), 0)
            await processes.stop()
            # signal送信と実際の終了の間にある競合を、固定sleepで隠さない。
            self.assertTrue(select.select([child_exit], [], [], 10)[0])
            stat = Path(f"/proc/{child_pid}/stat")
            if stat.exists():
                self.assertEqual(stat.read_text().split(") ", 1)[1].split()[0], "Z")
        finally:
            os.close(child_exit)
            try:
                os.kill(child_pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


if __name__ == "__main__":
    unittest.main()
