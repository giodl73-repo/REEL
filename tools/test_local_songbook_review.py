import hashlib
import tempfile
import unittest
from pathlib import Path
import xml.etree.ElementTree as ET
from local_songbook_review import checked, system_pages


class ReviewTests(unittest.TestCase):
    def grid(self,count):
        root=ET.Element('{http://www.w3.org/2000/svg}svg')
        for i in range(count):
            for d in (0,10,20,30,40):
                ET.SubElement(root,'{http://www.w3.org/2000/svg}line',{'class':'staff','x1':'80','x2':'1060','y1':str(80+150*i+d)})
        return root

    def test_every_system_paginated_once(self):
        self.assertEqual(system_pages(self.grid(9)),[(40,640),(640,1240),(1240,1390)])

    def test_reject_incomplete_staff(self):
        root=self.grid(2);root.remove(root[-1])
        with self.assertRaises(ValueError):system_pages(root)

    def test_reject_different_export_grid(self):
        root=self.grid(2);root[5].set('y1','231')
        with self.assertRaises(ValueError):system_pages(root)

    def test_corruption_fails(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source';p.write_bytes(b'abc')
            row=dict(path=str(p),sha256=hashlib.sha256(b'abc').hexdigest(),bytes=3)
            self.assertEqual(checked(row),p)
            p.write_bytes(b'abd')
            with self.assertRaises(ValueError):checked(row)


if __name__=='__main__':unittest.main()
