EAPI=8
DESCRIPTION="fixture package: pulls slotconfgroupnew (>=slotconflicttarget-2.0, reached first) plus slotconfgroupa/b/c (each <slotconflicttarget-2.0) -- an unsolvable slot conflict whose slot-1.0 instance has three parents sharing one collision reason, so the notice collapses them to one representative"
SLOT="0"
KEYWORDS="amd64"
RDEPEND="dev-libs/slotconfgroupnew dev-libs/slotconfgroupa dev-libs/slotconfgroupb dev-libs/slotconfgroupc"
