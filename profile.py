"""scx_slo testbed: two bare-metal nodes on a private LAN.

- server (10.10.1.1): memcached, batch jobs, sched_ext schedulers
- client (10.10.1.2): memtier_benchmark load generator

Each node gets an extra local disk at /mydata for kernel sources, Rust build
output and results. The root disk on most CloudLab node types is small.

Nothing is installed automatically. After the experiment is ready, follow
docs/CLOUDLAB.md: the server needs a kernel change and a reboot, which is
easier to watch by hand than from a startup script.

CloudLab picks up this file because it sits at the repository root
(repository-based profile). You can also paste it into the profile editor.
"""

import geni.portal as portal
import geni.rspec.pg as pg

pc = portal.Context()
pc.defineParameter(
    "hwtype", "Node hardware type", portal.ParameterType.STRING, "xl170",
    longDescription="Both nodes use this type. xl170 (Utah, 10 cores / 20 threads) "
    "is usually available; c220g5 (Wisconsin, 40 threads) gives more batch CPUs.",
)
pc.defineParameter(
    "image", "Disk image", portal.ParameterType.IMAGE,
    "urn:publicid:IDN+emulab.net+image+emulab-ops//UBUNTU24-64-STD",
)
pc.defineParameter(
    "datasize", "Size of /mydata on each node", portal.ParameterType.STRING, "100GB",
)
params = pc.bindParameters()

request = pc.makeRequestRSpec()
lan = request.LAN("lan")

for i, name in enumerate(["server", "client"]):
    node = request.RawPC(name)
    node.hardware_type = params.hwtype
    node.disk_image = params.image

    iface = node.addInterface("if1")
    iface.addAddress(pg.IPv4Address("10.10.1.%d" % (i + 1), "255.255.255.0"))
    lan.addInterface(iface)

    bs = node.Blockstore("bs-" + name, "/mydata")
    bs.size = params.datasize

pc.printRequestRSpec(request)
