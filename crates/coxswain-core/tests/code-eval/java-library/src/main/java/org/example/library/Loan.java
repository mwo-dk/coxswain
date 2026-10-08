package org.example.library;

import java.time.LocalDate;

/** A book lent to a member until `due`. */
public record Loan(String isbn, String member, LocalDate due) {}
