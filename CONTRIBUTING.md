---
description: Unificar efectos visuales/de render antes de implementar variantes separadas
alwaysApply: true
---

# Unificar efectos antes de separarlos

Cuando un mismo problema de presentación aparece en varios contextos (p. ej. héroe detrás de árbol vs bajo tierra, oclusión por hojas vs terreno), **propón un solo efecto compartido** antes de añadir passes, flags o curvas distintas por caso.

## Antes de implementar

1. Pregunta si los casos deben verse **igual** (un pipeline) o si realmente necesitan looks distintos.
2. Si deben verse igual: un pass / una regla / un flag; parametriza solo densidad o intensidad si hace falta.
3. No añadas discos, fades de burial, Bayer distinto, etc. “solo para bajo tierra” si el árbol ya usa otro X-ray — unifica primero.

## Señal de alarma

Dos sistemas que hacen “ver al jugador cuando algo lo tapa” → fusionar. Ejemplo evitado: screen-door de dosel + disco de pies / burial fade aparte.

## Excepción

Separar solo si el usuario pide explícitamente looks distintos, o si unificar rompe un caso de forma demostrable.
