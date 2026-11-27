#!/bin/bash
cat << 'PAYLOAD'
#!CSTL v5.0.0 MODE=A
META [encoder=Agent_GEMINI, produced_by=Gemini, sigma=0.88, PARENT_HASH=root]
INTENT_PAYLOAD [purpose=reply_fact, sender=bob, receiver=alice]
RELATION [type=confirms, subject=x, object=y]
---END---
PAYLOAD
