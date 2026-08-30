#!/bin/bash
cat << 'PAYLOAD'
#!CSTL v5.0.0 MODE=A
META [encoder=Agent_CLAUDE, produced_by=Claude, sigma=0.95, PARENT_HASH=root]
INTENT_PAYLOAD [purpose=query_fact, sender=alice, receiver=bob]
RELATION [type=equals, subject=x, object=y]
---END---
PAYLOAD
