package org.example.library;

import java.time.LocalDate;
import java.util.HashMap;
import java.util.Map;
import org.springframework.stereotype.Service;

/** Lending and returning books. */
@Service
public class LoanService {
    /** Days a member may keep a book. */
    static final int LOAN_DAYS = 14;
    /** Books a member may have at once. */
    static final int MAX_LOANS = 5;

    private final BookRepository books;
    private final FineCalculator fines;
    private final Map<String, Loan> open = new HashMap<>();

    public LoanService(BookRepository books, FineCalculator fines) {
        this.books = books;
        this.fines = fines;
    }

    /** Lends the book to the member, due back in LOAN_DAYS days; refused past MAX_LOANS. */
    public Loan lend(String isbn, String member) {
        long held = open.values().stream().filter(l -> l.member().equals(member)).count();
        if (held >= MAX_LOANS) {
            throw new IllegalStateException("a member may borrow at most " + MAX_LOANS + " books");
        }
        Book book = books.findById(isbn).orElseThrow();
        book.setOnLoan(true);
        books.save(book);
        Loan loan = new Loan(isbn, member, LocalDate.now().plusDays(LOAN_DAYS));
        open.put(isbn, loan);
        return loan;
    }

    /** Takes the book back and works out the fine for the days it is late. */
    public Fine giveBack(String isbn) {
        Loan loan = open.remove(isbn);
        Book book = books.findById(isbn).orElseThrow();
        book.setOnLoan(false);
        books.save(book);
        return fines.fineFor(loan, LocalDate.now());
    }
}
