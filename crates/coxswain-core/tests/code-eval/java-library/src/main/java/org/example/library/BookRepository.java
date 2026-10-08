package org.example.library;

import java.util.List;
import org.springframework.data.jpa.repository.JpaRepository;

/** Books in the database. */
public interface BookRepository extends JpaRepository<Book, String> {
    List<Book> findByAuthorContainingIgnoreCase(String author);
    List<Book> findByOnLoanFalse();
}
