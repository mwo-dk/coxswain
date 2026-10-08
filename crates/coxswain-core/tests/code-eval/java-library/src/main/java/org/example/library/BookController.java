package org.example.library;

import java.util.List;
import org.springframework.web.bind.annotation.*;

/** The REST API: books and their loans. */
@RestController
@RequestMapping("/books")
public class BookController {
    private final BookRepository books;
    private final LoanService loans;

    public BookController(BookRepository books, LoanService loans) {
        this.books = books;
        this.loans = loans;
    }

    @GetMapping
    public List<Book> all(@RequestParam(required = false) String author) {
        return author == null ? books.findAll() : books.findByAuthorContainingIgnoreCase(author);
    }

    @GetMapping("/available")
    public List<Book> available() {
        return books.findByOnLoanFalse();
    }

    @PostMapping("/{isbn}/loans")
    public Loan borrow(@PathVariable String isbn, @RequestParam String member) {
        return loans.lend(isbn, member);
    }

    @DeleteMapping("/{isbn}/loans")
    public Fine giveBack(@PathVariable String isbn) {
        return loans.giveBack(isbn);
    }
}
