package org.example.library;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.time.temporal.ChronoUnit;
import org.springframework.stereotype.Component;

/** Late fees: 0.25 a day after the due date, never more than 10.00 for one book. */
@Component
public class FineCalculator {
    private static final BigDecimal PER_DAY = new BigDecimal("0.25");
    private static final BigDecimal CAP = new BigDecimal("10.00");

    public Fine fineFor(Loan loan, LocalDate returned) {
        long late = ChronoUnit.DAYS.between(loan.due(), returned);
        if (late <= 0) {
            return new Fine(BigDecimal.ZERO);
        }
        return new Fine(PER_DAY.multiply(BigDecimal.valueOf(late)).min(CAP));
    }
}
