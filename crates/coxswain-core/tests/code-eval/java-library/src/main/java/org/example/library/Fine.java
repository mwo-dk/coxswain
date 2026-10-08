package org.example.library;

import java.math.BigDecimal;

/** What a member owes for a late book. */
public record Fine(BigDecimal amount) {}
