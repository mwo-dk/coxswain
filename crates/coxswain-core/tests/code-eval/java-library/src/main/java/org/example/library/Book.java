package org.example.library;

import jakarta.persistence.Entity;
import jakarta.persistence.Id;

/** A book on the shelves, by its ISBN. */
@Entity
public class Book {
    @Id
    private String isbn;
    private String title;
    private String author;
    private boolean onLoan;

    public String getIsbn() { return isbn; }
    public String getTitle() { return title; }
    public boolean isOnLoan() { return onLoan; }
    public void setOnLoan(boolean onLoan) { this.onLoan = onLoan; }
}
