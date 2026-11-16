import sys, xml.etree.ElementTree as ET
for tc in ET.parse(sys.argv[1]).getroot().iter('testcase'):
    st = 'fail' if tc.find('failure') is not None or tc.find('error') is not None else ('skip' if tc.find('skipped') is not None else 'pass')
    print(f"{tc.get('classname')}::{tc.get('name')} {st}")
