from pathlib import Path
import tempfile
import unittest
import wsl_python_worker as worker


class WslCommand(unittest.TestCase):
    def test_argument_array_preserves_spaces_without_shell_interpretation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/"wsl.exe").write_bytes(b"test");(root/"worker.py").write_bytes(b"test")
            args=worker.command(root,"wsl.exe","Ubuntu","/home/example/python",60,"worker.py",["a file.json"])
            self.assertEqual(args[1:4],["-d","Ubuntu","--exec"])
            self.assertEqual(args[-1],"a file.json")
            self.assertEqual(args[4:7],["/usr/bin/timeout","--kill-after=10","60"])

    def test_unbounded_escape_and_implicit_interpreter_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/"wsl.exe").write_bytes(b"test");(root/"worker.py").write_bytes(b"test")
            for script,python,seconds in [("../worker.py","/bin/python",60),("worker.py","python",60),("worker.py","/bin/python",0)]:
                with self.subTest(script=script,python=python,seconds=seconds),self.assertRaises(ValueError):
                    worker.command(root,"wsl.exe","Ubuntu",python,seconds,script,[])


if __name__=="__main__":unittest.main()
